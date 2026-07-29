import assert from "node:assert/strict";
import { validatePerformanceCheck } from "./readme-acceptance-contract.mjs";

// Catches requiring a noisy but valid performance check to pass instead of
// accepting BenchGuard's documented regression outcome.
assert.equal(
  validatePerformanceCheck({
    status: 0,
    stdout: "startup\n  PASS\n",
    stderr: "",
  }),
  "pass",
);
assert.equal(
  validatePerformanceCheck({
    status: 1,
    stdout: "startup\n  REGRESSION\n",
    stderr: "",
  }),
  "regression",
);

// Catches trusting the exit code without its matching report.
assert.throws(
  () =>
    validatePerformanceCheck({
      status: 0,
      stdout: "startup\n  REGRESSION\n",
      stderr: "",
    }),
  /exit 0 did not include a PASS report/,
);
assert.throws(
  () =>
    validatePerformanceCheck({
      status: 1,
      stdout: "startup\n  PASS\n",
      stderr: "",
    }),
  /exit 1 did not include a REGRESSION report/,
);

// Catches swallowing operational diagnostics or treating exit 2 as a
// performance result.
assert.throws(
  () =>
    validatePerformanceCheck({
      status: 2,
      stdout: "",
      stderr: "error: failed to launch benchmark command",
    }),
  /failed to launch benchmark command/,
);
assert.throws(
  () =>
    validatePerformanceCheck({
      status: null,
      signal: "SIGTERM",
      stdout: "",
      stderr: "terminated",
    }),
  /signal=SIGTERM[\s\S]*terminated/,
);
