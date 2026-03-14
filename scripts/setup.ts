#!/usr/bin/env npx tsx
/**
 * Linear Skill Setup Check
 *
 * Run this script to verify your Linear skill configuration:
 *   npx tsx setup.ts
 *
 * Or with silent mode (for postinstall):
 *   npx tsx setup.ts --silent
 */

import { existsSync, readFileSync } from 'fs';
import { execSync } from 'child_process';
import { homedir } from 'os';
import { join } from 'path';

interface SetupResult {
  ready: boolean;
  issues: string[];
  suggestions: string[];
}

const SILENT = process.argv.includes('--silent');

function log(...args: unknown[]) {
  if (!SILENT) console.log(...args);
}

function logError(...args: unknown[]) {
  console.error(...args);
}

async function checkLinearCredentials(): Promise<{ valid: boolean; issues: string[]; suggestions: string[]; authType?: string }> {
  const issues: string[] = [];
  const suggestions: string[] = [];

  const agentToken = process.env.LINEAR_AGENT_TOKEN;
  const apiKey = process.env.LINEAR_API_KEY;

  if (!agentToken && !apiKey) {
    issues.push('No Linear credentials set');
    suggestions.push(
      'Option A (Preferred): Set up an agent identity:',
      '  npx tsx scripts/oauth-setup.ts',
      '  Then set LINEAR_AGENT_TOKEN in your environment',
      '',
      'Option B: Use a personal API key:',
      '  1. Open Linear (https://linear.app)',
      '  2. Go to Settings -> Security & access -> Personal API keys',
      '  3. Click "Create key" and copy the key',
      '  4. Set LINEAR_API_KEY in your environment'
    );
    return { valid: false, issues, suggestions };
  }

  const token = agentToken || apiKey!;
  const authType = agentToken ? 'agent (OAuth)' : 'personal (API key)';

  // Validate personal key format
  if (!agentToken && apiKey && !apiKey.startsWith('lin_api_')) {
    issues.push('LINEAR_API_KEY has invalid format (should start with lin_api_)');
    suggestions.push('Regenerate your API key in Linear settings');
    return { valid: false, issues, suggestions };
  }

  // Test the token works
  try {
    const { LinearClient } = await import('@linear/sdk');
    const client = new LinearClient({ apiKey: token });
    const me = await client.viewer;
    const org = await me.organization;

    log(`  Auth type: ${authType}`);
    log(`  Authenticated as: ${me.name} (${(me as any).email || 'agent'})`);
    log(`  Organization: ${org?.name || 'Unknown'}`);

    if (!agentToken) {
      log('\n  [TIP] Set up an agent identity for bot-attributed actions:');
      log('    npx tsx scripts/oauth-setup.ts');
    }

    return { valid: true, issues: [], suggestions: [], authType };
  } catch (error) {
    const msg = error instanceof Error ? error.message : String(error);
    if (msg.includes('401') || msg.includes('unauthorized')) {
      issues.push(`${authType} token is invalid or expired`);
      suggestions.push(
        agentToken
          ? 'Re-run: npx tsx scripts/oauth-setup.ts'
          : 'Regenerate your API key in Linear settings'
      );
    } else {
      issues.push(`API connection failed: ${msg}`);
      suggestions.push('Check your network connection and try again');
    }
    return { valid: false, issues, suggestions };
  }
}

async function checkSdkInstalled(): Promise<{ installed: boolean; issues: string[]; suggestions: string[] }> {
  try {
    await import('@linear/sdk');
    return { installed: true, issues: [], suggestions: [] };
  } catch {
    return {
      installed: false,
      issues: ['@linear/sdk not installed'],
      suggestions: [
        'Install the Linear SDK:',
        '  npm install @linear/sdk  # Run from the skill directory'
      ]
    };
  }
}

function checkLinearCli(): { installed: boolean; path?: string } {
  try {
    const path = execSync('which linear 2>/dev/null', { encoding: 'utf8' }).trim();
    return { installed: true, path };
  } catch {
    return { installed: false };
  }
}

function checkMcpConfig(): { found: boolean; hasLinear: boolean; path?: string } {
  const searchPaths = [
    join(process.cwd(), '.mcp.json'),
    join(homedir(), '.mcp.json'),
    join(homedir(), '.claude', '.mcp.json')
  ];

  for (const mcpPath of searchPaths) {
    if (existsSync(mcpPath)) {
      try {
        const content = readFileSync(mcpPath, 'utf8');
        const config = JSON.parse(content);
        const hasLinear = !!(config.mcpServers?.linear || config.servers?.linear);
        return { found: true, hasLinear, path: mcpPath };
      } catch {
        return { found: true, hasLinear: false, path: mcpPath };
      }
    }
  }

  return { found: false, hasLinear: false };
}

async function runSetupCheck(): Promise<SetupResult> {
  const allIssues: string[] = [];
  const allSuggestions: string[] = [];

  log('\n========================================');
  log('  Linear Skill Setup Check');
  log('========================================\n');

  // 1. Check @linear/sdk
  log('Checking @linear/sdk...');
  const sdkResult = await checkSdkInstalled();
  if (sdkResult.installed) {
    log('  [OK] @linear/sdk is installed\n');
  } else {
    log('  [MISSING] @linear/sdk not found\n');
    allIssues.push(...sdkResult.issues);
    allSuggestions.push(...sdkResult.suggestions, '');
  }

  // 2. Check Linear credentials (only if SDK is installed)
  log('Checking Linear credentials...');
  if (sdkResult.installed) {
    const credResult = await checkLinearCredentials();
    if (credResult.valid) {
      log(`  [OK] Credentials valid (${credResult.authType})\n`);
    } else {
      log('  [MISSING/INVALID] Credentials issue\n');
      allIssues.push(...credResult.issues);
      allSuggestions.push(...credResult.suggestions, '');
    }
  } else {
    if (process.env.LINEAR_AGENT_TOKEN || process.env.LINEAR_API_KEY) {
      log('  [SET] Credentials set (cannot validate without SDK)\n');
    } else {
      log('  [MISSING] No Linear credentials set\n');
      allIssues.push('No Linear credentials set');
      allSuggestions.push(
        'After installing SDK, run: npx tsx scripts/oauth-setup.ts',
        ''
      );
    }
  }

  // 3. Check Linear CLI (optional)
  log('Checking Linear CLI (optional)...');
  const cliResult = checkLinearCli();
  if (cliResult.installed) {
    log(`  [OK] Linear CLI found at ${cliResult.path}\n`);
  } else {
    log('  [INFO] Linear CLI not installed (optional)\n');
    log('  To install: Download Linear Desktop from https://linear.app/download\n');
  }

  // 4. Check MCP configuration (optional)
  log('Checking MCP configuration (optional)...');
  const mcpResult = checkMcpConfig();
  if (mcpResult.found && mcpResult.hasLinear) {
    log(`  [OK] Linear MCP configured in ${mcpResult.path}\n`);
  } else if (mcpResult.found) {
    log(`  [INFO] .mcp.json found but Linear not configured\n`);
    log('  To add Linear MCP, add to your .mcp.json:\n');
    log('  {');
    log('    "mcpServers": {');
    log('      "linear": {');
    log('        "command": "npx",');
    log('        "args": ["-y", "linear-mcp-server"],');
    log('        "env": { "LINEAR_API_KEY": "${LINEAR_API_KEY}" }');
    log('      }');
    log('    }');
    log('  }\n');
  } else {
    log('  [INFO] No .mcp.json found (MCP tools will not be available)\n');
  }

  // Summary
  log('========================================');
  if (allIssues.length === 0) {
    log('  STATUS: Ready to use!');
    log('========================================\n');
    log('Quick commands:');
    log('  - Create initiative: npx tsx scripts/linear-ops.ts create-initiative "Name"');
    log('  - Update status:     node scripts/linear-helpers.mjs update-status Done 123');
    log('  - Query API:         npx tsx scripts/query.ts "query { viewer { name } }"');
    log('');
    return { ready: true, issues: [], suggestions: [] };
  } else {
    log('  STATUS: Setup incomplete');
    log('========================================\n');

    log('Issues found:');
    allIssues.forEach(issue => log(`  - ${issue}`));
    log('');

    log('To fix:');
    allSuggestions.forEach(s => log(`  ${s}`));

    return { ready: false, issues: allIssues, suggestions: allSuggestions };
  }
}

// Main
runSetupCheck()
  .then(result => {
    process.exit(result.ready ? 0 : 1);
  })
  .catch(error => {
    logError('Setup check failed:', error.message);
    process.exit(1);
  });
