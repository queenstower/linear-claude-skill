#!/usr/bin/env npx tsx
/**
 * Linear OAuth Agent Setup
 *
 * One-time script to obtain an OAuth access token for a Linear agent application
 * using the actor=app flow. This creates a dedicated agent user in the workspace.
 *
 * Required environment variables:
 *   LINEAR_OAUTH_CLIENT_ID     - OAuth application client ID
 *   LINEAR_OAUTH_CLIENT_SECRET - OAuth application client secret
 *
 * Usage:
 *   LINEAR_OAUTH_CLIENT_ID=xxx LINEAR_OAUTH_CLIENT_SECRET=xxx npx tsx scripts/oauth-setup.ts
 *
 * After successful auth, the script outputs the access token to store as LINEAR_AGENT_TOKEN.
 */

import http from 'http';
import { URL } from 'url';
import { execSync } from 'child_process';

const REDIRECT_PORT = 3456;
const REDIRECT_URI = `http://localhost:${REDIRECT_PORT}/callback`;
const LINEAR_AUTH_URL = 'https://linear.app/oauth/authorize';
const LINEAR_TOKEN_URL = 'https://api.linear.app/oauth/token';
const LINEAR_API_URL = 'https://api.linear.app/graphql';

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

function openBrowser(url: string): void {
  try {
    // Try xdg-open (Linux), then open (macOS), then start (Windows)
    const commands = ['xdg-open', 'open', 'start'];
    for (const cmd of commands) {
      try {
        execSync(`which ${cmd} 2>/dev/null`, { encoding: 'utf8' });
        execSync(`${cmd} "${url}"`, { stdio: 'ignore' });
        return;
      } catch {
        continue;
      }
    }
    console.log('\n[INFO] Could not auto-open browser.');
  } catch {
    console.log('\n[INFO] Could not auto-open browser.');
  }
}

async function main(): Promise<void> {
  const clientId = getRequiredEnv('LINEAR_OAUTH_CLIENT_ID');
  const clientSecret = getRequiredEnv('LINEAR_OAUTH_CLIENT_SECRET');

  console.log('\n========================================');
  console.log('  Linear Agent OAuth Setup');
  console.log('========================================\n');
  console.log('This will create a dedicated agent user in your Linear workspace.');
  console.log('You need workspace admin permissions to approve the installation.\n');

  const authUrl = buildAuthUrl(clientId);

  // Start local callback server
  const codePromise = new Promise<string>((resolve, reject) => {
    const server = http.createServer((req, res) => {
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
        res.end(`<html><body><h2>Authorization failed</h2><p>${error}</p><p>You can close this tab.</p></body></html>`);
        server.close();
        reject(new Error(`OAuth error: ${error}`));
        return;
      }

      if (!code) {
        res.writeHead(400, { 'Content-Type': 'text/html' });
        res.end('<html><body><h2>Missing authorization code</h2><p>You can close this tab.</p></body></html>');
        server.close();
        reject(new Error('No authorization code received'));
        return;
      }

      res.writeHead(200, { 'Content-Type': 'text/html' });
      res.end('<html><body><h2>Authorization successful!</h2><p>You can close this tab and return to the terminal.</p></body></html>');
      server.close();
      resolve(code);
    });

    server.listen(REDIRECT_PORT, () => {
      console.log(`Callback server listening on port ${REDIRECT_PORT}\n`);
    });

    server.on('error', (err: NodeJS.ErrnoException) => {
      if (err.code === 'EADDRINUSE') {
        reject(new Error(`Port ${REDIRECT_PORT} is already in use. Close the other process and try again.`));
      } else {
        reject(err);
      }
    });

    // Timeout after 5 minutes
    setTimeout(() => {
      server.close();
      reject(new Error('Timed out waiting for OAuth callback (5 minutes)'));
    }, 5 * 60 * 1000);
  });

  console.log('Opening browser for authorization...\n');
  console.log('If the browser does not open, visit this URL manually:\n');
  console.log(`  ${authUrl}\n`);

  openBrowser(authUrl);

  console.log('Waiting for authorization...\n');

  try {
    const code = await codePromise;
    console.log('Authorization code received. Exchanging for access token...\n');

    const tokenData = await exchangeCodeForToken(code, clientId, clientSecret);
    console.log('Token obtained successfully!\n');

    // Verify the token works
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

    console.log('========================================');
    console.log('  Save the following token securely:');
    console.log('========================================\n');
    console.log(`  LINEAR_AGENT_TOKEN=${tokenData.access_token}\n`);

    console.log('Add it to your environment:\n');
    console.log('  Option A: Claude Code environment (~/.claude/.env):');
    console.log(`    echo 'LINEAR_AGENT_TOKEN=${tokenData.access_token}' >> ~/.claude/.env\n`);
    console.log('  Option B: Shell profile (~/.zshrc or ~/.bashrc):');
    console.log(`    export LINEAR_AGENT_TOKEN="${tokenData.access_token}"\n`);

    console.log('The Linear skill will now use this agent identity instead of your personal API key.');
    console.log('Your personal LINEAR_API_KEY is still used as a fallback.\n');
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    console.error(`\n[ERROR] ${msg}\n`);
    process.exit(1);
  }
}

main();
