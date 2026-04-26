const SHA256_PATTERN = /^[0-9a-f]{64}$/;

export function normalizeSha256Input(value: string): string {
  return value.trim().toLowerCase();
}

export function validateOptionalSha256(value: string): string | null {
  const normalized = normalizeSha256Input(value);
  if (!normalized) return null;
  if (!SHA256_PATTERN.test(normalized)) {
    throw new Error('SHA-256 checksum must be 64 hexadecimal characters.');
  }
  return normalized;
}
