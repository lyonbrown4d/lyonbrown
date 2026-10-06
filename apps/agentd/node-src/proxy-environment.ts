import * as http from "node:http";

const PROXY_KEYS = [
  "http_proxy",
  "HTTP_PROXY",
  "https_proxy",
  "HTTPS_PROXY",
  "no_proxy",
  "NO_PROXY",
] as const;

type ProxyKey = (typeof PROXY_KEYS)[number];
type ProxyEnvironment = Readonly<Partial<Record<ProxyKey, string>>>;
type ConfigureGlobalProxy = (environment: ProxyEnvironment) => () => void;

export interface ProxyEnvironmentManager {
  acquire(): () => void;
}

export function createProxyEnvironmentManager(
  configure: ConfigureGlobalProxy,
  environment: Readonly<NodeJS.ProcessEnv>,
): ProxyEnvironmentManager {
  let references = 0;
  let restore: (() => void) | undefined;

  return Object.freeze({
    acquire(): () => void {
      if (references === 0) {
        try {
          restore = configure(selectProxyEnvironment(environment));
        } catch {
          throw new Error("PROXY_CONFIGURATION_INVALID");
        }
      }
      references += 1;
      let released = false;
      return (): void => {
        if (released) return;
        released = true;
        references -= 1;
        if (references !== 0) return;
        const action = restore;
        restore = undefined;
        action?.();
      };
    },
  });
}

const selectProxyEnvironment = (environment: Readonly<NodeJS.ProcessEnv>): ProxyEnvironment => {
  const selected: Partial<Record<ProxyKey, string>> = {};
  for (const key of PROXY_KEYS) {
    const value = environment[key];
    if (value !== undefined) selected[key] = value;
  }
  return selected;
};

type HttpWithEnvironmentProxy = typeof http & {
  setGlobalProxyFromEnv(environment?: ProxyEnvironment): () => void;
};

const configureNodeProxy: ConfigureGlobalProxy = (environment) =>
  (http as HttpWithEnvironmentProxy).setGlobalProxyFromEnv(environment);

export const agentdProxyEnvironment = createProxyEnvironmentManager(
  configureNodeProxy,
  process.env,
);
