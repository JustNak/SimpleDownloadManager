export function buildAddJobCommandArgs(url: string, expectedSha256?: string | null) {
  return {
    url,
    expectedSha256: expectedSha256 ? normalizeExpectedSha256(expectedSha256) : null,
  };
}

function normalizeExpectedSha256(value: string): string {
  const normalized = value.trim().toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(normalized)) {
    throw new Error('SHA-256 checksum must be 64 hexadecimal characters.');
  }
  return normalized;
}
