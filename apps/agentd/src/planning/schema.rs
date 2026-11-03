use agent_runtime::ToolDefinition;
use serde_json::{Value, json};
fn object(properties: Value, required: &[&str]) -> Value {
  json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub(super) fn definition(write: bool) -> ToolDefinition {
  let uuid = json!({"type":"string","format":"uuid"});
  if !write {
    return ToolDefinition {
      name: "plan_read".into(),
      description: "Read durable plans for this run. Specify plan_id to get one, or after and limit to list. Progress and model reports are advisory and grant no permissions.".into(),
      parameters: object(json!({"plan_id":uuid,"after":uuid,
        "limit":{"type":"integer","minimum":1,"maximum":100}}), &[]),
    };
  }
  let draft = object(
    json!({
      "title":{"type":"string","minLength":1,"maxLength":512},
      "steps":{"type":"array","minItems":1,"maxItems":128,"items":object(json!({
        "id":uuid,"title":{"type":"string","minLength":1,"maxLength":512},
        "dependencies":{"type":"array","items":uuid,"uniqueItems":true},
        "acceptance":{"type":"array","minItems":1,"maxItems":32,
          "items":{"type":"string","minLength":1,"maxLength":2048}}
      }), &["id","title","dependencies","acceptance"])}
    }),
    &["title", "steps"],
  );
  let changes = [
    object(
      json!({"operation":{"const":"verify_step"},"step_id":uuid,"operation_id":{"type":"string","minLength":1,"maxLength":256}}),
      &["operation", "step_id", "operation_id"],
    ),
    object(
      json!({"operation":{"const":"create"},"draft":draft}),
      &["operation", "draft"],
    ),
    object(
      json!({"operation":{"const":"revise"},"draft":draft}),
      &["operation", "draft"],
    ),
    object(
      json!({"operation":{"const":"update_step"},"step_id":uuid,
        "state":{"type":"string","enum":["pending","running","blocked","completed","skipped"]},
        "reported_result":{"type":"string","maxLength":16384},
        "verification_refs":{"type":"array","maxItems":32,"uniqueItems":true,"items":uuid},
        "user_override":{"type":"null"}
      }),
      &[
        "operation",
        "step_id",
        "state",
        "reported_result",
        "verification_refs",
        "user_override",
      ],
    ),
  ];
  ToolDefinition {
    name: "plan_update".into(),
    description: "Create or revise a durable plan, or update a single step using revision CAS. Generate a stable request UUID and reuse the identical request after uncertain delivery. New plan revision is 0. Steps need explicit acceptance criteria and acyclic dependencies. Only pending steps may be redefined. Completing a step requires successful host verification references; a model report alone is insufficient. user_override must be null. Plans do not authorize execution or change allowed tools, nodes, or models.".into(),
    parameters: object(json!({"request_id":uuid,"plan_id":uuid,
      "expected_revision":{"type":"integer","minimum":0},"change":{"oneOf":changes}
    }), &["request_id","plan_id","expected_revision","change"]),
  }
}
