// The frontend unit tests. Agent worktrees under .claude/ are not part of the
// tree, and the Claude observer mod's suites run under `claude plugin test`
// (they import the engine's own testing kit), not Vitest.
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    exclude: ["**/node_modules/**", "**/dist/**", ".claude/**", "target/**", "packages/provider-mod/**"],
  },
});
