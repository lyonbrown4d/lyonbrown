export const CI_GATES = ["preflight", "lint", "typecheck", "test", "build"] as const;

export type CiGate = (typeof CI_GATES)[number];
export type GateRunner = (gate: CiGate) => Promise<void>;

export async function runCiGate(run: GateRunner): Promise<void> {
  for (const gate of CI_GATES) {
    await run(gate);
  }
}
