/**
 * Shared Linear API utilities
 *
 * Common patterns extracted from Linear scripts for consistent
 * API key validation, entity lookups, and type definitions.
 */

import { LinearClient } from '@linear/sdk';
import { ensureValidToken } from './token-refresh.js';

// ============================================================================
// Types
// ============================================================================

/**
 * Health status for project and initiative updates
 */
export type HealthStatus = 'onTrack' | 'atRisk' | 'offTrack';

/**
 * Valid health status values
 */
export const VALID_HEALTH_VALUES: readonly HealthStatus[] = ['onTrack', 'atRisk', 'offTrack'] as const;

/**
 * Project information returned by findProjectByName
 */
export interface ProjectInfo {
  id: string;
  name: string;
  slugId: string;
}

/**
 * Initiative information returned by findInitiativeByName
 */
export interface InitiativeInfo {
  id: string;
  name: string;
  description?: string;
}

/**
 * Team information returned by findTeamByKey
 */
export interface TeamInfo {
  id: string;
  key: string;
  name: string;
}

// ============================================================================
// Type Guards
// ============================================================================

/**
 * Type guard to check if a string is a valid health status
 */
export function isValidHealth(value: string): value is HealthStatus {
  return VALID_HEALTH_VALUES.includes(value as HealthStatus);
}

// ============================================================================
// Client Initialization
// ============================================================================

/**
 * Create a LinearClient instance with the provided API key
 *
 * @param apiKey Linear API key
 * @returns LinearClient instance
 */
export function createLinearClient(apiKey: string): LinearClient {
  return new LinearClient({ apiKey });
}

/**
 * Get the best available Linear API token.
 *
 * Prefers LINEAR_AGENT_TOKEN (OAuth agent identity) over LINEAR_API_KEY (personal).
 * The agent token is obtained via the actor=app OAuth flow and creates actions
 * under the dedicated agent user instead of a personal account.
 *
 * @throws Error if neither token is set
 * @returns The API token string and which type it is
 */
export function getLinearToken(): { token: string; type: 'agent' | 'personal' } {
  const agentToken = process.env.LINEAR_AGENT_TOKEN;
  if (agentToken) {
    return { token: agentToken, type: 'agent' };
  }

  const apiKey = process.env.LINEAR_API_KEY;
  if (apiKey) {
    return { token: apiKey, type: 'personal' };
  }

  throw new Error(
    'No Linear credentials found. Set LINEAR_AGENT_TOKEN (preferred) or LINEAR_API_KEY.\n' +
    'Run: npx tsx scripts/oauth-setup.ts  — to set up the agent identity'
  );
}

/**
 * Get a LinearClient instance with token validation.
 *
 * Prefers LINEAR_AGENT_TOKEN (OAuth agent) over LINEAR_API_KEY (personal).
 *
 * @throws Error if no Linear credentials are set
 * @returns LinearClient instance
 */
export function getLinearClient(): LinearClient {
  const { token } = getLinearToken();
  return createLinearClient(token);
}

/**
 * Get a valid Linear API token, refreshing if the OAuth token has expired.
 *
 * For agent tokens (OAuth), this tests the token against the API and
 * automatically refreshes it using LINEAR_OAUTH_CLIENT_ID/SECRET if expired.
 * Personal API keys are returned as-is (they don't expire).
 *
 * @throws Error if no credentials are set or refresh fails
 * @returns The validated API token string and its type
 */
export async function getValidLinearToken(): Promise<{ token: string; type: 'agent' | 'personal' }> {
  const result = getLinearToken();

  if (result.type === 'personal') {
    return result;
  }

  // Agent token — validate and refresh if expired
  const validToken = await ensureValidToken(result.token);
  return { token: validToken, type: 'agent' };
}

/**
 * Get a LinearClient with a validated (and possibly refreshed) token.
 *
 * Async variant of getLinearClient that auto-refreshes expired OAuth tokens.
 *
 * @throws Error if no credentials are set or refresh fails
 * @returns LinearClient instance with a valid token
 */
export async function getValidLinearClient(): Promise<LinearClient> {
  const { token } = await getValidLinearToken();
  return createLinearClient(token);
}

// ============================================================================
// Entity Lookups
// ============================================================================

/**
 * Find a project by name with exact-match preference
 *
 * Searches for projects with case-insensitive partial matching.
 * If multiple matches are found, prefers exact match (case-insensitive).
 *
 * @param client LinearClient instance
 * @param name Project name to search for
 * @returns ProjectInfo or null if not found
 */
export async function findProjectByName(
  client: LinearClient,
  name: string
): Promise<ProjectInfo | null> {
  const projects = await client.projects({
    filter: {
      name: { containsIgnoreCase: name }
    },
    first: 10
  });

  if (projects.nodes.length === 0) {
    return null;
  }

  // Prefer exact match (case-insensitive)
  const exactMatch = projects.nodes.find(
    p => p.name.toLowerCase() === name.toLowerCase()
  );

  const project = exactMatch || projects.nodes[0];
  return {
    id: project.id,
    name: project.name,
    slugId: project.slugId
  };
}

/**
 * Find an initiative by name with exact-match preference
 *
 * Searches for initiatives with case-insensitive partial matching.
 * If multiple matches are found, prefers exact match (case-insensitive).
 *
 * @param client LinearClient instance
 * @param name Initiative name to search for
 * @returns InitiativeInfo or null if not found
 */
export async function findInitiativeByName(
  client: LinearClient,
  name: string
): Promise<InitiativeInfo | null> {
  const query = `
    query FindInitiative($filter: InitiativeFilter!) {
      initiatives(filter: $filter, first: 10) {
        nodes {
          id
          name
          description
        }
      }
    }
  `;

  const result = await client.client.rawRequest(query, {
    filter: {
      name: { containsIgnoreCase: name }
    }
  });

  const data = result.data as { initiatives: { nodes: InitiativeInfo[] } };
  const initiatives = data.initiatives.nodes;

  if (initiatives.length === 0) {
    return null;
  }

  // Prefer exact match (case-insensitive)
  const exactMatch = initiatives.find(
    i => i.name.toLowerCase() === name.toLowerCase()
  );

  return exactMatch || initiatives[0];
}

/**
 * Find a team by its key (e.g., "ENG", "PRODUCT")
 *
 * @param client LinearClient instance
 * @param key Team key to search for (case-insensitive)
 * @returns TeamInfo or null if not found
 */
export async function findTeamByKey(
  client: LinearClient,
  key: string
): Promise<TeamInfo | null> {
  const query = `
    query TeamByKey($filter: TeamFilter!) {
      teams(filter: $filter, first: 1) {
        nodes {
          id
          key
          name
        }
      }
    }
  `;

  const result = await client.client.rawRequest(query, {
    filter: {
      key: { eq: key.toUpperCase() }
    }
  });

  const data = result.data as {
    teams: {
      nodes: TeamInfo[];
    };
  };

  return data.teams.nodes[0] || null;
}

/**
 * Find a team by name with case-insensitive matching
 *
 * Searches for teams with case-insensitive partial matching.
 * If multiple matches are found, prefers exact match (case-insensitive).
 *
 * @param client LinearClient instance
 * @param name Team name to search for
 * @returns TeamInfo or null if not found
 */
export async function findTeamByName(
  client: LinearClient,
  name: string
): Promise<TeamInfo | null> {
  const query = `
    query TeamByName($filter: TeamFilter!) {
      teams(filter: $filter, first: 10) {
        nodes {
          id
          key
          name
        }
      }
    }
  `;

  const result = await client.client.rawRequest(query, {
    filter: {
      name: { containsIgnoreCase: name }
    }
  });

  const data = result.data as {
    teams: {
      nodes: TeamInfo[];
    };
  };

  const teams = data.teams.nodes;

  if (teams.length === 0) {
    return null;
  }

  // Prefer exact match (case-insensitive)
  const exactMatch = teams.find(
    t => t.name.toLowerCase() === name.toLowerCase()
  );

  return exactMatch || teams[0];
}
