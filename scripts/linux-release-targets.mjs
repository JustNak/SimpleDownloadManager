import path from 'node:path';

export const linuxReleaseTargets = Object.freeze({
  x64: Object.freeze({
    name: 'x64',
    rustTarget: 'x86_64-unknown-linux-gnu',
    updaterPlatform: 'linux-x86_64',
    appImageArch: 'amd64',
    debArch: 'amd64',
    rpmArch: 'x86_64',
  }),
});

export const linuxReleaseTargetList = Object.freeze([
  linuxReleaseTargets.x64,
]);

export function linuxReleaseTargetForRustTarget(rustTarget) {
  const target = linuxReleaseTargetList.find((candidate) => candidate.rustTarget === rustTarget);
  if (!target) {
    throw new Error(`Unsupported Linux release Rust target: ${rustTarget}`);
  }
  return target;
}

export function linuxReleaseTargetForName(name) {
  const normalized = String(name ?? '').trim().toLowerCase();
  if (['x64', 'amd64', 'x86_64', linuxReleaseTargets.x64.rustTarget].includes(normalized)) {
    return linuxReleaseTargets.x64;
  }
  throw new Error(`Unsupported Linux release target: ${name}`);
}

export function resolveLinuxReleaseTargets(names) {
  if (!names || names.length === 0) {
    return [...linuxReleaseTargetList];
  }

  const values = Array.isArray(names) ? names : [names];
  return values
    .flatMap((value) => String(value).split(','))
    .map((value) => value.trim())
    .filter(Boolean)
    .map(linuxReleaseTargetForName);
}

export function linuxAppImageName(version, target = linuxReleaseTargets.x64) {
  return `Simple Download Manager_${version}_${target.appImageArch}.AppImage`;
}

export function linuxBundleArtifactNames(version, target = linuxReleaseTargets.x64) {
  return {
    appimage: linuxAppImageName(version, target),
    deb: `Simple Download Manager_${version}_${target.debArch}.deb`,
    rpm: `Simple Download Manager-${version}-1.${target.rpmArch}.rpm`,
  };
}

export function tauriLinuxTargetBundleDir(root, target) {
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

export function nativeHostLinuxTargetDir(root, target) {
  return path.join(
    root,
    'apps',
    'native-host',
    'target',
    target.rustTarget,
    'release',
  );
}
