import assert from 'node:assert/strict';

let publishUpdaterAlpha;
try {
  publishUpdaterAlpha = await import('../../../scripts/publish-updater-alpha.mjs');
} catch (error) {
  assert.fail(`Updater publish helper should be importable without running gh: ${error instanceof Error ? error.message : error}`);
}

const {
  assertGitHubCliAvailable,
  isMissingGitHubCliError,
  missingGitHubCliMessage,
} = publishUpdaterAlpha;

assert.equal(
  isMissingGitHubCliError({ code: 'ENOENT', path: 'gh', syscall: 'spawn gh' }),
  true,
  'publish script should recognize a missing GitHub CLI executable',
);

assert.match(
  missingGitHubCliMessage(),
  /GitHub CLI \(gh\) was not found on PATH/,
  'missing gh should produce a clear installation-focused error',
);

assert.match(
  missingGitHubCliMessage(),
  /gh auth login/,
  'missing gh guidance should mention authentication before publishing',
);

await assert.rejects(
  () => assertGitHubCliAvailable(async () => {
    const error = new Error('spawn gh ENOENT');
    error.code = 'ENOENT';
    error.path = 'gh';
    error.syscall = 'spawn gh';
    throw error;
  }),
  /GitHub CLI \(gh\) was not found on PATH/,
  'preflight should convert missing executable errors into release guidance',
);
