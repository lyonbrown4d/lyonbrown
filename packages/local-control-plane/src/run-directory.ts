import { DiscoveryStore } from "./discovery-store.js";
import {
  createSecurityPolicy,
  prepareSecureDirectory,
  type SecurityOptions,
  type SecurityPolicy,
} from "./security.js";

export class SecureRunDirectory {
  public readonly discovery: DiscoveryStore;

  public constructor(
    public readonly path: string,
    readonly securityPolicy: SecurityPolicy,
  ) {
    this.discovery = new DiscoveryStore(path, securityPolicy);
    Object.freeze(this);
  }
}

export async function openSecureRunDirectory(
  path: string,
  options: SecurityOptions = {},
): Promise<SecureRunDirectory> {
  const policy = createSecurityPolicy(options);
  const securePath = await prepareSecureDirectory(path, policy);
  return new SecureRunDirectory(securePath, policy);
}
