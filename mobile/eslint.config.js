const { defineConfig } = require("eslint/config");
const js = require("@eslint/js");
const tseslint = require("typescript-eslint");
const reactHooks = require("eslint-plugin-react-hooks");

const typedFiles = ["**/*.{ts,tsx}"];
const forTypedFiles = (configs) =>
  configs.map((config) => ({ ...config, files: typedFiles }));

const practicalCoreRules = {
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
  eqeqeq: ["error", "always", { null: "ignore" }],
  "no-eq-null": "off",
  "no-void": ["error", { allowAsStatement: true }],
  complexity: ["error", 50],
  "max-lines": [
    "error",
    { max: 1500, skipBlankLines: true, skipComments: true },
  ],
  "max-lines-per-function": [
    "error",
    { max: 750, skipBlankLines: true, skipComments: true, IIFEs: true },
  ],
  "max-params": ["error", 8],
  "max-statements": ["error", 100],
  "new-cap": ["error", { capIsNewExceptions: ["Pan"] }],
};

module.exports = defineConfig([
  js.configs.all,
  ...forTypedFiles(tseslint.configs.all),
  { ...reactHooks.configs.flat["recommended-latest"], files: typedFiles },
  {
    rules: practicalCoreRules,
  },
  {
    files: ["**/*.js"],
    languageOptions: {
      globals: {
        module: "readonly",
        process: "readonly",
        require: "readonly",
        __dirname: "readonly",
      },
    },
  },
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: __dirname,
      },
    },
    rules: {
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
      // React hooks expose callback methods that are already context-free.
      "@typescript-eslint/unbound-method": "off",
      "no-console": "error",
      // React Native Animated/Reanimated values and worklet closures are ref-like,
      // but intentionally read while constructing gestures and animated styles.
      "react-hooks/refs": "off",
    },
  },
]);
