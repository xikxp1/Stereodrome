import js from "@eslint/js";
import ts from "typescript-eslint";
import svelte from "eslint-plugin-svelte";
import prettier from "eslint-config-prettier";
import globals from "globals";
import { fileURLToPath } from "node:url";

const tsconfigRootDir = fileURLToPath(new URL(".", import.meta.url));
const typedFiles = ["src/**/*.ts", "vite.config.ts"];
const forTypedFiles = (configs) =>
  configs.map((config) => ({ ...config, files: typedFiles }));

const practicalCoreRules = {
  // Formatting and declaration layout are owned by Prettier or do not improve correctness.
  "arrow-body-style": "off",
  "capitalized-comments": "off",
  camelcase: "off",
  "default-case": "off",
  "dot-notation": "off",
  "func-names": "off",
  "func-style": "off",
  "id-length": "off",
  "init-declarations": "off",
  "no-continue": "off",
  "no-implicit-coercion": "off",
  "no-inline-comments": "off",
  "no-lonely-if": "off",
  "no-magic-numbers": "off",
  "no-negated-condition": "off",
  "no-nested-ternary": "off",
  "no-plusplus": "off",
  "no-ternary": "off",
  "no-undefined": "off",
  "no-use-before-define": "off",
  "one-var": "off",
  "prefer-arrow-callback": "off",
  "prefer-destructuring": "off",
  "prefer-named-capture-group": "off",
  "require-atomic-updates": "off",
  "require-unicode-regexp": "off",
  "sort-imports": "off",
  "sort-keys": "off",
  // Nullish equality intentionally covers both null and undefined.
  eqeqeq: ["error", "always", { null: "ignore" }],
  "no-eq-null": "off",
  // Explicitly handled promises use the void operator as a statement marker.
  "no-void": ["error", { allowAsStatement: true }],
  // Keep size checks useful without forcing unrelated architectural rewrites.
  complexity: ["error", 50],
  "max-lines": [
    "error",
    { max: 1500, skipBlankLines: true, skipComments: true },
  ],
  "max-lines-per-function": [
    "error",
    { max: 300, skipBlankLines: true, skipComments: true, IIFEs: true },
  ],
  "max-params": ["error", 8],
  "max-statements": ["error", 100],
};

/** @type {import('eslint').Linter.Config[]} */
export default [
  js.configs.all,
  ...forTypedFiles(ts.configs.all),
  ...svelte.configs["flat/all"],
  prettier,
  ...svelte.configs["flat/prettier"],
  {
    rules: practicalCoreRules,
  },
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.node,
      },
    },
  },
  {
    files: typedFiles,
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir,
      },
    },
  },
  {
    files: ["**/*.svelte"],
    languageOptions: {
      parserOptions: {
        parser: ts.parser,
        tsconfigRootDir,
      },
    },
    rules: {
      // TypeScript handles declarations embedded in Svelte prop signatures.
      "no-unused-vars": "off",
      // Svelte rune destructuring remains reactive without direct reassignment.
      "prefer-const": "off",
    },
  },
  {
    files: ["**/*.svelte.ts", "**/*.svelte.js"],
    languageOptions: {
      parser: svelte.parser,
      parserOptions: {
        parser: ts.parser,
        projectService: true,
        tsconfigRootDir,
        extraFileExtensions: [".svelte"],
      },
    },
  },
  {
    files: typedFiles,
    rules: {
      // These rules enforce subjective declaration style rather than safety.
      "@typescript-eslint/class-methods-use-this": "off",
      "@typescript-eslint/consistent-type-definitions": "off",
      "@typescript-eslint/explicit-function-return-type": "off",
      "@typescript-eslint/explicit-module-boundary-types": "off",
      "@typescript-eslint/explicit-member-accessibility": "off",
      "@typescript-eslint/dot-notation": "off",
      "@typescript-eslint/member-ordering": "off",
      "@typescript-eslint/method-signature-style": "off",
      "@typescript-eslint/no-inferrable-types": "off",
      "@typescript-eslint/no-magic-numbers": "off",
      "@typescript-eslint/no-use-before-define": "off",
      "@typescript-eslint/prefer-destructuring": "off",
      "@typescript-eslint/prefer-readonly-parameter-types": "off",
      "@typescript-eslint/promise-function-async": "off",
      "@typescript-eslint/max-params": ["error", { max: 8 }],
      // Rust/Tauri wire payloads intentionally use snake_case properties.
      "@typescript-eslint/naming-convention": [
        "error",
        { selector: "typeLike", format: ["PascalCase"] },
        {
          selector: "variable",
          format: ["camelCase", "UPPER_CASE", "PascalCase"],
          leadingUnderscore: "allow",
        },
        {
          selector: ["parameter", "parameterProperty"],
          format: ["camelCase"],
          leadingUnderscore: "allow",
        },
        {
          selector: "function",
          format: ["camelCase", "PascalCase"],
          leadingUnderscore: "allow",
        },
        { selector: "property", format: null },
      ],
      "@typescript-eslint/restrict-template-expressions": [
        "error",
        { allowBoolean: true, allowNullish: true, allowNumber: true },
      ],
      "@typescript-eslint/switch-exhaustiveness-check": "error",
      "@typescript-eslint/unbound-method": ["error", { ignoreStatic: true }],
    },
  },
  {
    rules: {
      // Tailwind/daisyUI classes and dynamic class construction are not visible to this rule.
      "svelte/no-unused-class-name": "off",
      // These rules impose markup/CSS ordering preferences that Prettier does not own.
      "svelte/block-lang": ["error", { script: "ts", style: null }],
      "svelte/consistent-selector-style": "off",
      "svelte/no-inline-styles": "off",
      "svelte/prefer-class-directive": "off",
      "svelte/prefer-style-directive": "off",
      "svelte/sort-attributes": "off",
      // The all preset still exposes these legacy aliases; their replacements remain enabled.
      "svelte/@typescript-eslint/no-unnecessary-condition": "off",
      "svelte/no-dynamic-slot-name": "off",
      "svelte/no-goto-without-base": "off",
      "svelte/no-navigation-without-base": "off",
    },
  },
  {
    files: ["scripts/set-version.mjs"],
    rules: {
      // This CLI intentionally communicates results directly to its caller.
      "no-console": "off",
    },
  },
  {
    ignores: [
      "build/",
      ".svelte-kit/",
      "dist/",
      "node_modules/",
      "target/",
      "src-tauri/target/",
      "mobile/",
      "mobile/node_modules/",
      "mobile/.expo/",
      "mobile/android/",
      "mobile/ios/",
    ],
  },
];
