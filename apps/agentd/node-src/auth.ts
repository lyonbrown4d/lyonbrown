import {
  Metadata,
  ServerInterceptingCall,
  ServerListenerBuilder,
  status,
  type ServerInterceptor,
} from "@grpc/grpc-js";
import type { SecretToken } from "@lyonbrown/local-control-plane";
import { AUTHORIZATION_METADATA_KEY, authorizeBearer } from "@lyonbrown/protocol";

const AUTHENTICATION_REQUIRED = "authentication required";

export function createAuthorizationInterceptor(token: SecretToken): ServerInterceptor {
  const expectedToken = token.exposeForTransport();
  return (_method, call) =>
    new ServerInterceptingCall(call, {
      start(next) {
        next(
          new ServerListenerBuilder()
            .withOnReceiveMetadata((metadata, forward) => {
              const values = metadata.get(AUTHORIZATION_METADATA_KEY);
              const authorization = values.length === 1 ? values[0] : undefined;
              const result = authorizeBearer(authorization, expectedToken);
              if (result.ok) {
                forward(metadata);
                return;
              }
              call.sendStatus({
                code: status.UNAUTHENTICATED,
                details: AUTHENTICATION_REQUIRED,
                metadata: new Metadata(),
              });
            })
            .build(),
        );
      },
    });
}
