import eslint from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";

const typeScriptFiles = [
  "scripts/**/*.ts",
  "vitest.config.ts",
  "apps/docs/astro.config.ts",
  "apps/docs/src/**/*.ts",
  "apps/docs/tests/**/*.ts",
  "packages/**/*.ts",
];

export default tseslint.config(
  {
    ignores: ["coverage/**", "dist/**", "node_modules/**", "target/**"],
  },
  eslint.configs.recommended,
  ...tseslint.configs.strictTypeChecked.map((config) => ({ ...config, files: typeScriptFiles })),
  ...tseslint.configs.stylisticTypeChecked.map((config) => ({ ...config, files: typeScriptFiles })),
  {
    files: typeScriptFiles,
    languageOptions: {
      globals: globals.node,
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      "@typescript-eslint/consistent-type-imports": "error",
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/no-floating-promises": "error",
    },
  },
  {
    files: ["eslint.config.mjs"],
    languageOptions: {
      globals: globals.node,
    },
  },
);
