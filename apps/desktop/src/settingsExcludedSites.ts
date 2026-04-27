import { normalizeExcludedHostPattern } from '@myapp/protocol';

export interface AddExcludedHostsResult {
  hosts: string[];
  addedHosts: string[];
  duplicateHosts: string[];
}

export function normalizeHostInput(value: string): string {
  return normalizeExcludedHostPattern(value);
}

export function parseExcludedHostInput(value: string): string[] {
  return value
    .split(/[\r\n,]+/)
    .map((item) => normalizeHostInput(item))
    .filter(Boolean);
}

export function addExcludedHosts(currentHosts: string[], candidates: string[]): AddExcludedHostsResult {
  const hosts = [...currentHosts];
  const existing = new Set(hosts);
  const addedHosts: string[] = [];
  const duplicateHosts: string[] = [];

  for (const candidate of candidates) {
    const normalizedHost = normalizeHostInput(candidate);
    if (!normalizedHost) continue;

    if (existing.has(normalizedHost)) {
      if (!duplicateHosts.includes(normalizedHost)) {
        duplicateHosts.push(normalizedHost);
      }
      continue;
    }

    existing.add(normalizedHost);
    hosts.push(normalizedHost);
    addedHosts.push(normalizedHost);
  }

  return { hosts, addedHosts, duplicateHosts };
}

export function removeExcludedHost(hosts: string[], host: string): string[] {
  return hosts.filter((candidate) => candidate !== host);
}

export function filterExcludedHosts(hosts: string[], query: string): string[] {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) return hosts;
  return hosts.filter((host) => host.includes(normalizedQuery));
}

export function formatExcludedSitesSummary(hosts: string[]): string {
  if (hosts.length === 0) return 'No excluded sites';
  if (hosts.length === 1) return '1 excluded site';
  return `${hosts.length} excluded sites`;
}
