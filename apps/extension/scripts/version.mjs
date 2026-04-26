export function extensionManifestVersion(packageVersion) {
  const normalizedVersion = String(packageVersion ?? '').trim();
  const releaseVersion = normalizedVersion.split('+')[0].split('-')[0];
  if (!/^\d+(?:\.\d+){1,3}$/.test(releaseVersion)) {
    throw new Error(`Invalid extension package version: ${packageVersion}`);
  }
  return releaseVersion;
}

export function extensionDisplayVersion(packageVersion) {
  const displayVersion = String(packageVersion ?? '').trim();
  if (!displayVersion) {
    throw new Error('Extension package version is required.');
  }
  return displayVersion;
}

export function extensionVersionsFromPackage(packageJson) {
  const packageVersion = packageJson?.version;
  return {
    browserExtensionVersion: extensionManifestVersion(packageVersion),
    displayVersion: extensionDisplayVersion(packageVersion),
  };
}
