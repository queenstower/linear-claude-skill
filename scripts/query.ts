#!/usr/bin/env npx tsx

/**
 * Execute ad-hoc GraphQL queries against the Linear API
 *
 * Usage:
 *   LINEAR_API_KEY=lin_api_xxx npx tsx query.ts "query { viewer { id name } }"
 *   LINEAR_API_KEY=lin_api_xxx npx tsx query.ts "query { viewer { id name } }" '{"var": "value"}'
 */

import { LinearClient } from '@linear/sdk';
import { getValidLinearToken } from './lib/linear-utils.js';

interface GraphQLErrorResponse {
  errors: Array<{
    message: string;
    locations?: Array<{ line: number; column: number }>;
    path?: Array<string | number>;
  }>;
}

function hasGraphQLErrors(error: unknown): error is Error & GraphQLErrorResponse {
  return (
    error instanceof Error &&
    'errors' in error &&
    Array.isArray((error as Record<string, unknown>).errors)
  );
}

async function main() {
  let token: string;
  try {
    const result = await getValidLinearToken();
    token = result.token;
    if (result.type === 'personal') {
      console.error('[INFO] Using personal API key. Set LINEAR_AGENT_TOKEN for agent identity.\n');
    }
  } catch (err) {
    console.error(`\n[ERROR] ${err instanceof Error ? err.message : err}\n`);
    console.error('Or run the full setup check:');
    console.error('  npx tsx setup.ts\n');
    process.exit(1);
  }

  const query = process.argv[2];
  const variablesArg = process.argv[3];

  if (!query) {
    console.error('Error: Query argument is required');
    console.error('');
    console.error('Usage:');
    console.error('  npx tsx query.ts "query { viewer { id name } }"');
    process.exit(1);
  }

  let variables = {};
  if (variablesArg) {
    try {
      variables = JSON.parse(variablesArg);
    } catch (_error) {
      console.error('Error: Variables must be valid JSON');
      console.error(`Received: ${variablesArg}`);
      process.exit(1);
    }
  }

  const client = new LinearClient({ apiKey: token });

  try {
    const result = await client.client.rawRequest(query, variables);
    console.log(JSON.stringify(result.data, null, 2));
  } catch (error) {
    console.error('Error executing query:');

    if (hasGraphQLErrors(error)) {
      console.error(error.message);
      console.error('\nGraphQL Errors:');
      console.error(JSON.stringify(error.errors, null, 2));
    } else if (error instanceof Error) {
      console.error(error.message);
    } else {
      console.error(error);
    }

    process.exit(1);
  }
}

main();
