export type OpenJevQuestion =
  | {
      readonly type: "choice";
      readonly instructions: string;
      readonly options: readonly string[];
    }
  | {
      readonly type: "score";
      readonly instructions: string;
      readonly options: readonly string[];
    }
  | { readonly type: "noul"; readonly instructions: string };

export interface OpenJevRuntime {
  readonly decide: (
    state: string,
    questions: readonly OpenJevQuestion[],
    options: { readonly truncation: "error" },
  ) => Promise<unknown>;
  readonly dispose: () => Promise<void>;
}
