import path from 'node:path';

export const windowsReleaseTargets = Object.freeze({
  x64: Object.freeze({
    name: 'x64',
    rustTarget: 'x86_64-pc-windows-msvc',
    installerArch: 'x64',
    updaterPlatform: 'windows-x86_64',
    sevenZipResources: Object.freeze([
      Object.freeze({ source: 'resources/bin/7z.exe', destination: 'resources/bin/7z.exe' }),
      Object.freeze({ source: 'resources/bin/7z.dll', destination: 'resources/bin/7z.dll' }),
      Object.freeze({ source: 'resources/bin/7zip-LICENSE.txt', destination: 'resources/bin/7zip-LICENSE.txt' }),
    ]),
  }),
  arm64: Object.freeze({
    name: 'arm64',
    rustTarget: 'aarch64-pc-windows-msvc',
    installerArch: 'arm64',
    updaterPlatform: 'windows-aarch64',
    sevenZipResources: Object.freeze([
      Object.freeze({ source: 'resources/bin/windows-arm64/7z.exe', destination: 'resources/bin/7z.exe' }),
      Object.freeze({ source: 'resources/bin/windows-arm64/7z.dll', destination: 'resources/bin/7z.dll' }),
      Object.freeze({ source: 'resources/bin/7zip-LICENSE.txt', destination: 'resources/bin/7zip-LICENSE.txt' }),
    ]),
  }),
});

export const windowsReleaseTargetList = Object.freeze([
  windowsReleaseTargets.x64,
  windowsReleaseTargets.arm64,
]);

export function windowsReleaseTargetForRustTarget(rustTarget) {
  const target = windowsReleaseTargetList.find((candidate) => candidate.rustTarget === rustTarget);
  if (!target) {
    throw new Error(`Unsupported Windows release Rust target: ${rustTarget}`);
  }
  return target;
}

export function windowsReleaseTargetForName(name) {
  const normalized = String(name ?? '').trim().toLowerCase();
  if (['x64', 'amd64', 'x86_64', windowsReleaseTargets.x64.rustTarget].includes(normalized)) {
    return windowsReleaseTargets.x64;
  }
  if (['arm64', 'aarch64', windowsReleaseTargets.arm64.rustTarget].includes(normalized)) {
    return windowsReleaseTargets.arm64;
  }
  throw new Error(`Unsupported Windows release target: ${name}`);
}

export function resolveWindowsReleaseTargets(names) {
  if (!names || names.length === 0) {
    return [...windowsReleaseTargetList];
  }

  const values = Array.isArray(names) ? names : [names];
  return values
    .flatMap((value) => String(value).split(','))
    .map((value) => value.trim())
    .filter(Boolean)
    .map(windowsReleaseTargetForName);
}

export function windowsInstallerName(version, target = windowsReleaseTargets.x64) {
  return `Simple Download Manager_${version}_${target.installerArch}-setup.exe`;
}

export function tauriTargetBundleDir(root, target) {
  return path.join(
    root,
    'apps',
    'desktop',
    'src-tauri',
    'target',
    target.rustTarget,
    'release',
    'bundle',
  );
}

export function nativeHostTargetDir(root, target) {
  return path.join(
    root,
    'apps',
    'native-host',
    'target',
    target.rustTarget,
    'release',
  );
}
