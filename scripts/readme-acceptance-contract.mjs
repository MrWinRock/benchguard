export function validatePerformanceCheck(result) {
  if (result.status === 0) {
    if (hasReportStatus(result.stdout, "PASS")) return "pass";
    throw outcomeError("exit 0 did not include a PASS report", result);
  }

  if (result.status === 1) {
    if (hasReportStatus(result.stdout, "REGRESSION")) return "regression";
    throw outcomeError("exit 1 did not include a REGRESSION report", result);
  }

  throw outcomeError("README check produced an operational outcome", result);
}

function hasReportStatus(output, status) {
  return new RegExp(`(?:^|\\s)${status}(?:\\s|$)`).test(output ?? "");
}

function outcomeError(reason, result) {
  return new Error(
    [
      reason,
      `status=${result.status ?? "null"} signal=${result.signal ?? "none"}`,
      `stdout:\n${result.stdout ?? ""}`,
      `stderr:\n${result.stderr ?? ""}`,
    ].join("\n"),
  );
}
