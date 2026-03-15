/**
 * Linear OAuth token refresh and persistence
 *
 * Handles automatic refresh of expired OAuth access tokens using
 * the refresh token flow. Persists new tokens to the .env file
 * so subsequent invocations use the fresh token.
 *
 * Required env vars for refresh:
 *   LINEAR_OAUTH_CLIENT_ID     - OAuth application client ID
 *   LINEAR_OAUTH_CLIENT_SECRET - OAuth application client secret
 *   LINEAR_REFRESH_TOKEN       - Refresh token from initial OAuth flow
 */

import { readFileSync, writeFileSync, existsSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const LINEAR_TOKEN_URL = 'https://api.linear.app/oauth/token';
const LINEAR_API_URL = 'https://api.linear.app/graphql';

interface TokenRefreshResponse {
  access_token: string;
  refresh_token?: string;
  token_type: string;
  expires_in?: number;
  scope?: string;
}

/**
 * Resolve the .env file path for the linear-claude-skill directory.
 */
function getEnvFilePath(): string {
  const __filename = fileURLToPath(import.meta.url);
  const __dirname = dirname(__filename);
  // lib/ is inside scripts/, so go up two levels to reach the skill root
  return resolve(__dirname, '..', '..', '.env');
}

/**
 * Test whether an access token is valid by calling the Linear API.
 */
export async function isTokenValid(token: string): Promise<boolean> {
  try {
    const response = await fetch(LINEAR_API_URL, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({ query: '{ viewer { id } }' }),
    });
    if (!response.ok) return false;
    const data = (await response.json()) as { errors?: unknown[] };
    return !data.errors;
  } catch {
    return false;
  }
}

/**
 * Refresh the OAuth access token using the refresh token.
 *
 * @returns New access token and (optionally) a rotated refresh token
 * @throws Error if required env vars are missing or refresh fails
 */
export async function refreshAccessToken(): Promise<TokenRefreshResponse> {
  const clientId = process.env.LINEAR_OAUTH_CLIENT_ID;
  const clientSecret = process.env.LINEAR_OAUTH_CLIENT_SECRET;
  const refreshToken = process.env.LINEAR_REFRESH_TOKEN;

  if (!clientId || !clientSecret) {
    throw new Error(
      'Cannot refresh token: LINEAR_OAUTH_CLIENT_ID and LINEAR_OAUTH_CLIENT_SECRET are required.\n' +
        'Set these env vars to enable automatic token refresh.'
    );
  }

  if (!refreshToken) {
    throw new Error(
      'Cannot refresh token: LINEAR_REFRESH_TOKEN is not set.\n' +
        'Re-run the OAuth setup to obtain a refresh token:\n' +
        '  LINEAR_OAUTH_CLIENT_ID=xxx LINEAR_OAUTH_CLIENT_SECRET=xxx npx tsx scripts/oauth-setup.ts'
    );
  }

  const response = await fetch(LINEAR_TOKEN_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      grant_type: 'refresh_token',
      refresh_token: refreshToken,
      client_id: clientId,
      client_secret: clientSecret,
    }).toString(),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(
      `Token refresh failed (${response.status}): ${text}\n` +
        'The refresh token may be invalid or revoked. Re-run OAuth setup:\n' +
        '  npx tsx scripts/oauth-setup.ts'
    );
  }

  return (await response.json()) as TokenRefreshResponse;
}

/**
 * Update a key=value pair in the .env file.
 * If the key exists, its value is replaced. Otherwise a new line is appended.
 */
function upsertEnvVar(envPath: string, key: string, value: string): void {
  let content = existsSync(envPath) ? readFileSync(envPath, 'utf-8') : '';

  const regex = new RegExp(`^${key}=.*$`, 'm');
  if (regex.test(content)) {
    content = content.replace(regex, `${key}=${value}`);
  } else {
    content = content.trimEnd() + `\n${key}=${value}\n`;
  }

  writeFileSync(envPath, content, 'utf-8');
}

/**
 * Persist refreshed tokens to the .env file and update process.env.
 */
export function persistTokens(
  accessToken: string,
  refreshToken?: string
): void {
  const envPath = getEnvFilePath();

  upsertEnvVar(envPath, 'LINEAR_AGENT_TOKEN', accessToken);
  process.env.LINEAR_AGENT_TOKEN = accessToken;

  if (refreshToken) {
    upsertEnvVar(envPath, 'LINEAR_REFRESH_TOKEN', refreshToken);
    process.env.LINEAR_REFRESH_TOKEN = refreshToken;
  }
}

/**
 * Ensure we have a valid Linear OAuth token.
 *
 * 1. Read the current token from env
 * 2. Test it against the API
 * 3. If expired (401), attempt a refresh using client credentials
 * 4. Persist the new tokens to .env for future invocations
 *
 * @returns A valid access token
 * @throws Error if no token is available and refresh fails
 */
export async function ensureValidToken(
  currentToken: string
): Promise<string> {
  // Test the current token first
  if (await isTokenValid(currentToken)) {
    return currentToken;
  }

  console.error('[INFO] OAuth token expired, attempting refresh...');

  const tokenData = await refreshAccessToken();
  persistTokens(tokenData.access_token, tokenData.refresh_token);

  // Verify the new token works
  if (!(await isTokenValid(tokenData.access_token))) {
    throw new Error(
      'Refreshed token failed validation. The OAuth app may have been revoked.\n' +
        'Re-run OAuth setup: npx tsx scripts/oauth-setup.ts'
    );
  }

  console.error('[INFO] Token refreshed successfully.');
  return tokenData.access_token;
}
