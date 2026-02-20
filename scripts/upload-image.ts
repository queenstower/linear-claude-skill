#!/usr/bin/env bun
/**
 * Upload image to Linear and optionally attach to an issue
 *
 * Usage:
 *   bun run upload-image.ts <image-path> [issue-id]
 *
 * Examples:
 *   bun run upload-image.ts ~/Desktop/screenshot.png
 *   bun run upload-image.ts /tmp/diagram.jpg TRE-123
 *   bun run upload-image.ts /tmp/mockup.png TRE-123 "Here's the mockup"
 *
 * Output:
 *   - Prints the asset URL (for embedding in descriptions/comments)
 *   - If issue-id provided, adds the image as a comment on that issue
 */

import { LinearClient } from '@linear/sdk';
import { readFileSync, statSync } from 'fs';
import { basename, extname } from 'path';

const API_KEY = process.env.LINEAR_API_KEY;
if (!API_KEY) {
  console.error('[ERROR] LINEAR_API_KEY environment variable is required');
  process.exit(1);
}

const args = process.argv.slice(2);
if (args.length === 0) {
  console.error('Usage: bun run upload-image.ts <image-path> [issue-id] [comment-text]');
  console.error('Example: bun run upload-image.ts ~/Desktop/screenshot.png TRE-123');
  process.exit(1);
}

const imagePath = args[0].replace(/^~/, process.env.HOME || '');
const issueIdentifier = args[1];
const commentText = args[2] || '';

// Detect content type
const ext = extname(imagePath).toLowerCase();
const contentTypeMap: Record<string, string> = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.svg': 'image/svg+xml',
  '.pdf': 'application/pdf',
};
const contentType = contentTypeMap[ext] || 'application/octet-stream';

async function main() {
  // Read file
  let fileData: Buffer;
  let fileSize: number;
  try {
    fileData = readFileSync(imagePath);
    fileSize = statSync(imagePath).size;
  } catch (err) {
    console.error(`[ERROR] Cannot read file: ${imagePath}`);
    console.error(String(err));
    process.exit(1);
  }

  const filename = basename(imagePath);
  console.log(`Uploading: ${filename} (${(fileSize / 1024).toFixed(1)} KB, ${contentType})`);

  const client = new LinearClient({ apiKey: API_KEY });

  // Step 1: Request upload URL from Linear
  const uploadResult = await client.fileUpload(contentType, filename, fileSize);
  const uploadFile = uploadResult.uploadFile;

  if (!uploadFile) {
    console.error('[ERROR] Failed to get upload URL from Linear');
    process.exit(1);
  }

  const { uploadUrl, assetUrl, headers } = uploadFile;

  // Step 2: Upload file to the signed URL
  const uploadHeaders: Record<string, string> = {
    'Content-Type': contentType,
    'Cache-Control': 'public, max-age=31536000',
  };
  for (const h of headers) {
    uploadHeaders[h.key] = h.value;
  }

  const uploadResponse = await fetch(uploadUrl, {
    method: 'PUT',
    headers: uploadHeaders,
    body: fileData,
  });

  if (!uploadResponse.ok) {
    console.error(`[ERROR] Upload failed: ${uploadResponse.status} ${uploadResponse.statusText}`);
    process.exit(1);
  }

  console.log('\n[SUCCESS] Image uploaded!');
  console.log(`  Asset URL: ${assetUrl}`);
  console.log(`  Markdown:  ![${filename}](${assetUrl})`);

  // Step 3: Optionally attach to an issue as a comment
  if (issueIdentifier) {
    console.log(`\nAttaching to issue ${issueIdentifier}...`);

    // Find issue by identifier
    const issueNumber = parseInt(issueIdentifier.replace(/^[A-Z]+-/, ''), 10);
    const teamKey = issueIdentifier.replace(/-\d+$/, '');

    const issues = await client.issues({
      filter: {
        number: { eq: issueNumber },
        team: { key: { eq: teamKey } },
      },
    });

    if (issues.nodes.length === 0) {
      console.error(`[ERROR] Issue ${issueIdentifier} not found`);
      process.exit(1);
    }

    const issue = issues.nodes[0];
    const body = commentText
      ? `${commentText}\n\n![${filename}](${assetUrl})`
      : `![${filename}](${assetUrl})`;

    await client.createComment({
      issueId: issue.id,
      body,
    });

    console.log(`[SUCCESS] Image attached to ${issueIdentifier} as a comment`);
    console.log(`  Issue URL: ${issue.url}`);
  }
}

main().catch(err => {
  console.error('[ERROR]', err.message || err);
  process.exit(1);
});
