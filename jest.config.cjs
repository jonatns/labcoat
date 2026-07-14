module.exports = {
  testEnvironment: "node",
  testMatch: ["**/__tests__/**/*.test.ts"],
  moduleNameMapper: {
    "^@/(.*)\\.js$": "<rootDir>/src/$1",
    "^@/(.*)$": "<rootDir>/src/$1",
    "^(\\.{1,2}/.*)\\.js$": "$1",
  },
  // nanoid ships ESM-only; let ts-jest downlevel it to CJS for the test runtime.
  transform: {
    "^.+\\.tsx?$": ["ts-jest", { tsconfig: { allowJs: true } }],
    "^.+\\.m?js$": ["ts-jest", { tsconfig: { allowJs: true } }],
  },
  transformIgnorePatterns: ["node_modules/(?!nanoid)"],
};
