import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const defaultsSource = readFileSync(new URL('../src/defaultSettings.ts', import.meta.url), 'utf8');

assert.match(
  defaultsSource,
  /export function defaultBulkDownloadDirectory\(downloadDirectory: string\)[\s\S]*replace\(\s*\/\[\\\\\/\]\+\$\/[\s\S]*\$\{separator\}Bulk/,
  'bulk default output directory should live under the configured download root while preserving separator style',
);

assert.match(
  defaultsSource,
  /bulk:\s*\{[\s\S]*outputDirectory:\s*defaultBulkDownloadDirectory\(downloadDirectory\)[\s\S]*maxConcurrentDownloads:\s*2[\s\S]*speedLimitKibPerSecond:\s*0[\s\S]*downloadPerformanceMode:\s*'balanced'[\s\S]*hosterFairnessMode:\s*'adaptive'[\s\S]*hosterAccelerationMode:\s*'safe'[\s\S]*autoRetryOverrideEnabled:\s*false[\s\S]*autoRetryAttempts:\s*3[\s\S]*startBehavior:\s*'review_then_start'[\s\S]*expandActiveRowsByDefault:\s*false/,
  'default settings should expose the bulk download section with independent runtime controls and review-first behavior',
);
