#!/usr/bin/env npx tsx
/**
 * Linear OAuth Agent Setup
 *
 * Obtains an OAuth access token for a Linear agent application using the
 * actor=app flow. Creates a dedicated agent user in the workspace.
 *
 * Supports two modes (raced automatically):
 *   1. Local callback — browser redirects to localhost (works when browser is local)
 *   2. Manual paste   — copy the code from the redirect URL and paste it here
 *                        (works on headless/remote servers)
 *
 * Required environment variables:
 *   LINEAR_OAUTH_CLIENT_ID     - OAuth application client ID
 *   LINEAR_OAUTH_CLIENT_SECRET - OAuth application client secret
 *
 * Usage:
 *   export $(grep -v '^#' .env | xargs)
 *   npx tsx scripts/oauth-setup.ts
 */

import http from 'http';
import { URL } from 'url';
import { createInterface } from 'readline';
import { execSync } from 'child_process';
import { persistTokens } from './lib/token-refresh.js';

const REDIRECT_PORT = 3456;
const REDIRECT_URI = `http://localhost:${REDIRECT_PORT}/callback`;
const LINEAR_AUTH_URL = 'https://linear.app/oauth/authorize';
const LINEAR_TOKEN_URL = 'https://api.linear.app/oauth/token';
const LINEAR_API_URL = 'https://api.linear.app/graphql';

const LINEAR_REVOKE_URL = 'https://api.linear.app/oauth/revoke';

const SCOPES = [
  'read',
  'write',
  'app:assignable',
  'app:mentionable',
  'initiative:read',
  'initiative:write',
];

interface TokenResponse {
  access_token: string;
  refresh_token?: string;
  token_type: string;
  expires_in?: number;
  scope: string;
}

interface ViewerResponse {
  data: {
    viewer: {
      id: string;
      name: string;
      displayName: string;
      active: boolean;
    };
  };
}

function getRequiredEnv(name: string): string {
  const value = process.env[name];
  if (!value) {
    console.error(`\n[ERROR] ${name} environment variable is required\n`);
    console.error('Usage:');
    console.error(`  ${name}=xxx npx tsx scripts/oauth-setup.ts\n`);
    process.exit(1);
  }
  return value;
}

function buildAuthUrl(clientId: string): string {
  const params = new URLSearchParams({
    client_id: clientId,
    redirect_uri: REDIRECT_URI,
    response_type: 'code',
    scope: SCOPES.join(','),
    actor: 'app',
    prompt: 'consent',
  });
  return `${LINEAR_AUTH_URL}?${params.toString()}`;
}

async function exchangeCodeForToken(
  code: string,
  clientId: string,
  clientSecret: string
): Promise<TokenResponse> {
  const response = await fetch(LINEAR_TOKEN_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      grant_type: 'authorization_code',
      code,
      redirect_uri: REDIRECT_URI,
      client_id: clientId,
      client_secret: clientSecret,
    }).toString(),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`Token exchange failed (${response.status}): ${text}`);
  }

  return response.json() as Promise<TokenResponse>;
}

async function verifyToken(accessToken: string): Promise<ViewerResponse> {
  const response = await fetch(LINEAR_API_URL, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify({
      query: `query { viewer { id name displayName active } }`,
    }),
  });

  if (!response.ok) {
    throw new Error(`API verification failed (${response.status})`);
  }

  return response.json() as Promise<ViewerResponse>;
}

function openBrowser(url: string): boolean {
  const commands = ['xdg-open', 'open', 'start'];
  for (const cmd of commands) {
    try {
      execSync(`which ${cmd} 2>/dev/null`, { encoding: 'utf8' });
      execSync(`${cmd} "${url}"`, { stdio: 'ignore' });
      return true;
    } catch {
      continue;
    }
  }
  return false;
}

/**
 * Revoke an existing OAuth token via the Linear API.
 * This allows a fresh authorization flow to issue a new token (with refresh token).
 */
async function revokeToken(token: string): Promise<boolean> {
  try {
    const response = await fetch(LINEAR_REVOKE_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams({ token }).toString(),
    });
    return response.ok;
  } catch {
    return false;
  }
}

/**
 * Extract the authorization code from user input.
 * Accepts either a bare code string or a full callback URL containing ?code=...
 */
function extractCodeFromInput(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;

  // Check if it's a URL with ?code= parameter
  try {
    const url = new URL(trimmed);
    const code = url.searchParams.get('code');
    if (code) return code;
  } catch {
    // Not a URL — treat as bare code
  }

  // Accept bare code (alphanumeric + hyphens, typical OAuth code format)
  if (/^[\w-]+$/.test(trimmed)) {
    return trimmed;
  }

  return null;
}

/**
 * Wait for the auth code via local HTTP callback server.
 * Returns a promise + a cleanup function to stop the server.
 */
function waitForCallback(): { promise: Promise<string>; cleanup: () => void } {
  let server: http.Server | null = null;

  const promise = new Promise<string>((resolve, reject) => {
    server = http.createServer((req, res) => {
      if (!req.url?.startsWith('/callback')) {
        res.writeHead(404);
        res.end('Not found');
        return;
      }

      const url = new URL(req.url, `http://localhost:${REDIRECT_PORT}`);
      const code = url.searchParams.get('code');
      const error = url.searchParams.get('error');

      if (error) {
        res.writeHead(200, { 'Content-Type': 'text/html' });
        res.end(`<html><body><h2>Authorization failed</h2><p>${error}</p></body></html>`);
        server?.close();
        reject(new Error(`OAuth error: ${error}`));
        return;
      }

      if (!code) {
        res.writeHead(400, { 'Content-Type': 'text/html' });
        res.end('<html><body><h2>Missing authorization code</h2></body></html>');
        server?.close();
        reject(new Error('No authorization code received'));
        return;
      }

      res.writeHead(200, { 'Content-Type': 'text/html' });
      res.end('<html><body><h2>Authorization successful!</h2><p>You can close this tab.</p></body></html>');
      server?.close();
      resolve(code);
    });

    server.listen(REDIRECT_PORT, () => {
      // Server started silently
    });

    server.on('error', () => {
      // Port unavailable — callback mode won't work, but manual paste still will
    });

    setTimeout(() => {
      server?.close();
      reject(new Error('Timed out waiting for callback'));
    }, 5 * 60 * 1000);
  });

  return {
    promise,
    cleanup: () => { server?.close(); },
  };
}

/**
 * Wait for the auth code via manual paste on stdin.
 * Returns a promise + a cleanup function to close readline.
 */
function waitForManualPaste(): { promise: Promise<string>; cleanup: () => void } {
  const rl = createInterface({ input: process.stdin, output: process.stdout });

  const promise = new Promise<string>((resolve, reject) => {
    rl.question('\n  Paste the code (or full redirect URL) here: ', (answer) => {
      const code = extractCodeFromInput(answer);
      if (code) {
        resolve(code);
      } else {
        reject(new Error('Could not extract authorization code from input'));
      }
      rl.close();
    });

    setTimeout(() => {
      rl.close();
      reject(new Error('Timed out waiting for manual input'));
    }, 5 * 60 * 1000);
  });

  return {
    promise,
    cleanup: () => { rl.close(); },
  };
}

async function main(): Promise<void> {
  const clientId = getRequiredEnv('LINEAR_OAUTH_CLIENT_ID');
  const clientSecret = getRequiredEnv('LINEAR_OAUTH_CLIENT_SECRET');

  console.log('\n========================================');
  console.log('  Linear Agent OAuth Setup');
  console.log('========================================\n');
  console.log('This will create a dedicated agent user in your Linear workspace.');
  console.log('You need workspace admin permissions to approve the installation.\n');

  // Revoke existing token so Linear shows the consent screen again
  const existingToken = process.env.LINEAR_AGENT_TOKEN;
  if (existingToken) {
    console.log('Revoking existing token to allow fresh authorization...');
    const revoked = await revokeToken(existingToken);
    if (revoked) {
      console.log('  Existing token revoked successfully.\n');
    } else {
      console.log('  Could not revoke existing token (may already be expired).');
      console.log('  Proceeding with authorization anyway.\n');
      console.log('  If Linear says "already authorized" without redirecting,');
      console.log('  go to Linear Settings > Authorized Applications > revoke the app manually.\n');
    }
  }

  const authUrl = buildAuthUrl(clientId);

  // Try to open browser
  const browserOpened = openBrowser(authUrl);

  console.log('Open this URL in your browser to authorize:\n');
  console.log(`  ${authUrl}\n`);

  if (browserOpened) {
    console.log('  (browser opened automatically)\n');
  }

  console.log('After authorizing, either:');
  console.log('  A) The browser will redirect back automatically (if local)');
  console.log('  B) Copy the code from the redirect URL and paste it below\n');
  console.log('     The redirect URL looks like:');
  console.log('     http://localhost:3456/callback?code=<THE_CODE_TO_COPY>\n');

  // Race: local callback vs manual paste — first one wins
  const callback = waitForCallback();
  const manual = waitForManualPaste();

  let code: string;
  try {
    code = await Promise.race([callback.promise, manual.promise]);
  } finally {
    callback.cleanup();
    manual.cleanup();
  }

  console.log('\nAuthorization code received. Exchanging for access token...\n');

  try {
    const tokenData = await exchangeCodeForToken(code, clientId, clientSecret);
    console.log('Token obtained successfully!\n');

    console.log('Verifying agent identity...\n');
    const viewer = await verifyToken(tokenData.access_token);
    const agent = viewer.data.viewer;

    console.log('========================================');
    console.log('  Agent User Created Successfully!');
    console.log('========================================\n');
    console.log(`  Agent ID:   ${agent.id}`);
    console.log(`  Agent Name: ${agent.name || agent.displayName}`);
    console.log(`  Active:     ${agent.active}`);
    console.log(`  Scopes:     ${tokenData.scope}\n`);

    // Auto-persist tokens to .env file
    persistTokens(tokenData.access_token, tokenData.refresh_token);

    console.log('========================================');
    console.log('  Tokens saved to .env');
    console.log('========================================\n');
    console.log(`  LINEAR_AGENT_TOKEN=${tokenData.access_token}\n`);

    if (tokenData.refresh_token) {
      console.log(`  LINEAR_REFRESH_TOKEN=${tokenData.refresh_token}`);
      console.log('  (auto-refresh enabled — token will renew automatically when expired)\n');
    } else {
      console.log('  [WARN] No refresh token received. Token auto-refresh will not be available.');
      console.log('  You may need to re-run this setup when the access token expires.\n');
    }

    console.log('The Linear skill will now use this agent identity instead of your personal API key.');
    console.log('Your personal LINEAR_API_KEY is still used as a fallback.\n');
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    console.error(`\n[ERROR] ${msg}\n`);
    process.exit(1);
  }
}

main();
