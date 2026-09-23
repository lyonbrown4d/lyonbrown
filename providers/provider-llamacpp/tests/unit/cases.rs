    #[test]
    fn descriptor_canonicalizes_a_regular_gguf_file() {
        let fixture = GgufFixture::new();
        let model = GgufModel::new("model", &fixture.0, 1_024, 64).expect("descriptor");

        assert!(model.path().is_absolute());
        assert_eq!(
            model.path(),
            fixture.0.canonicalize().expect("canonical path")
        );
    }

    #[test]
    fn descriptor_rejects_invalid_identity_path_and_limits() {
        let fixture = GgufFixture::new();
        let directory = fixture.0.parent().expect("fixture parent");

        assert!(GgufModel::new(" ", &fixture.0, 1, 1).is_err());
        assert!(GgufModel::new("model", directory, 1, 1).is_err());
        assert!(GgufModel::new("model", directory.join("missing.gguf"), 1, 1).is_err());
        assert!(GgufModel::new("model", fixture.0.with_extension("bin"), 1, 1).is_err());
        assert!(GgufModel::new("model", &fixture.0, 0, 1).is_err());
        assert!(GgufModel::new("model", &fixture.0, 1, 0).is_err());
        assert!(GgufModel::new("model", &fixture.0, 16, 17).is_err());
    }

    #[test]
    fn config_rejects_invalid_runtime_values() {
        let fixture = GgufFixture::new();
        let mut candidate = config(&fixture);
        candidate.threads = 0;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.batch_threads = 0;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.batch_size = 0;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.batch_size = MAX_BATCH_SIZE + 1;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.batch_size = candidate.model.context_size() + 1;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.temperature = f32::NAN;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.temperature = -0.1;
        assert!(candidate.validate().is_err());

        candidate = config(&fixture);
        candidate.gpu_layers = i32::MAX as u32 + 1;
        assert!(candidate.validate().is_err());
    }

    #[test]
    fn prompt_messages_preserve_deterministic_role_order() {
        let request = request("local-test");
        assert_eq!(
            prompt_messages(&request).expect("prompt messages"),
            vec![
                PromptMessage::new("system", "Be concise."),
                PromptMessage::new("user", "hello"),
                PromptMessage::new("assistant", "Hi!"),
                PromptMessage::new("tool", "tool output"),
            ]
        );
    }

    #[test]
    fn prompt_chunks_are_bounded_and_cover_every_token() {
        assert_eq!(chunk_ranges(10, 4), vec![0..4, 4..8, 8..10]);
        assert_eq!(chunk_ranges(3, 4), vec![0..3]);
    }

    #[tokio::test]
    async fn queued_inference_is_cancelled_when_receiver_is_dropped() {
        let gate = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let held = gate.clone().lock_owned().await;
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let waiter = tokio::spawn(acquire_inference_gate(gate, sender));

        drop(receiver);
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("cancelled waiter finishes")
            .expect("waiter task does not panic");
        assert!(result.is_none());
        drop(held);
    }

    #[tokio::test]
    async fn concurrent_inference_is_rejected_at_bounded_admission() {
        let admission = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let permit = try_admit_inference(admission.clone()).expect("first request is admitted");

        let error = try_admit_inference(admission.clone()).expect_err("second request is busy");
        assert!(matches!(error, ProviderError::Busy));

        drop(permit);
        assert!(try_admit_inference(admission).is_ok());
    }

    #[test]
    fn request_validation_rejects_unsupported_modes_and_wrong_model() {
        let fixture = GgufFixture::new();
        let config = config(&fixture);

        let error = validate_request(&config, &request("other")).expect_err("wrong model");
        assert!(matches!(error, ProviderError::InvalidRequest(_)));

        let mut with_tools = request("local-test");
        with_tools.tools.push(ToolDefinition {
            name: "lookup".into(),
            description: "lookup".into(),
            input_schema: serde_json::json!({"type": "object"}),
            source: ToolSource::BuiltIn,
            risk: RiskLevel::Read,
            requires_secrets: Vec::new(),
        });
        assert!(validate_request(&config, &with_tools).is_ok());

        let mut json = request("local-test");
        json.json_mode = true;
        assert!(validate_request(&config, &json).is_err());

        let mut thinking = request("local-test");
        thinking.thinking = Some(true);
        assert!(validate_request(&config, &thinking).is_err());

        let mut empty_stop = request("local-test");
        empty_stop.stop = vec![String::new()];
        assert!(validate_request(&config, &empty_stop).is_err());
    }

    #[test]
    fn capability_reports_one_local_streaming_worker() {
        let fixture = GgufFixture::new();
        let capability = capabilities(&config(&fixture));

        assert_eq!(capability.provider, "llamacpp");
        assert_eq!(capability.model, "local-test");
        assert_eq!(capability.context_window, 2_048);
        assert_eq!(capability.max_output_tokens, 256);
        assert!(capability.supports_streaming);
        assert!(capability.supports_tools);
        assert!(!capability.supports_json);
        assert!(!capability.supports_thinking);
    }

    #[test]
    fn natural_eog_ends_the_turn() {
        assert_eq!(
            termination_reason(Termination::Eog),
            agent_core::StopReason::EndTurn
        );
    }

    #[test]
    fn configured_stop_reports_stop() {
        assert_eq!(
            termination_reason(Termination::StopString),
            agent_core::StopReason::Stop
        );
    }

    #[test]
    fn exhausted_output_budget_reports_max_tokens() {
        assert_eq!(
            termination_reason(Termination::OutputLimit),
            agent_core::StopReason::MaxTokens
        );
    }

    #[cfg(not(feature = "llama-cpp"))]
    #[test]
    fn constructor_is_explicitly_unavailable_without_native_feature() {
        let fixture = GgufFixture::new();
        let error = LlamaCppProvider::new(config(&fixture)).expect_err("feature is disabled");

        assert!(matches!(error, ProviderError::Unavailable));
    }

    #[cfg(feature = "llama-cpp")]
    #[test]
    fn invalid_gguf_fails_cleanly_during_construction() {
        let fixture = GgufFixture::new();
        let error = LlamaCppProvider::new(config(&fixture)).expect_err("invalid GGUF");

        assert!(matches!(error, ProviderError::Native(_)));
    }

    #[cfg(feature = "llama-cpp")]
    #[test]
    fn stop_buffer_handles_a_stop_string_split_across_tokens() {
        let stops = ["STOP".into()];
        let mut buffer = StopBuffer::new(&stops);

        assert_eq!(buffer.push("answer ST"), (Some("answer ".into()), false));
        assert_eq!(buffer.push("OP ignored"), (None, true));
        assert_eq!(buffer.finish(), None);
    }

    #[cfg(feature = "llama-cpp")]
    #[tokio::test]
    #[ignore = "requires a real GGUF path in LYONBROWN_GGUF_MODEL"]
    async fn live_completion_smoke_test() {
        let path = std::env::var_os("LYONBROWN_GGUF_MODEL")
            .expect("set LYONBROWN_GGUF_MODEL to a local GGUF file");
        let config = LlamaCppConfig {
            model: GgufModel::new("live", path, 2_048, 32).expect("valid model descriptor"),
            threads: 2,
            batch_threads: 2,
            batch_size: 512,
            gpu_layers: 0,
            temperature: 0.0,
            seed: 42,
        };
        let provider = LlamaCppProvider::new(config).expect("load live model");
        let mut stream = provider
            .stream(ModelRequest {
                model: "live".into(),
                system: None,
                messages: vec![message(Role::User, "Reply with one short greeting.")],
                tools: Vec::new(),
                max_tokens: 16,
                temperature: Some(0.0),
                json_mode: false,
                thinking: None,
                stop: Vec::new(),
            })
            .await
            .expect("start completion");
        let mut saw_delta = false;
        let mut saw_done = false;
        while let Some(event) = stream.events.next().await {
            match event.expect("completion event") {
                ModelEvent::Delta(delta) => saw_delta |= !delta.is_empty(),
                ModelEvent::Done { .. } => saw_done = true,
                ModelEvent::Usage(_) | ModelEvent::ToolCall(_) => {}
            }
        }
        assert!(saw_delta, "model returned no text");
        assert!(saw_done, "model stream did not finish");
    }
