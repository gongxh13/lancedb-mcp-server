#!/usr/bin/env node
const { spawn } = require('child_process');
const path = require('path');

const binaryName = process.platform === 'win32' ? 'lancedb-mcp-server.exe' : 'lancedb-mcp-server';
const binaryPath = path.join(__dirname, binaryName);

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: 'inherit'
});

child.on('exit', (code) => {
  process.exit(code);
});
