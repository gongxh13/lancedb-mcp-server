const fs = require('fs');
const path = require('path');
const https = require('https');
const { execSync } = require('child_process');
const packageJson = require('../package.json');

const platform = process.platform;
const arch = process.arch;

const SUPPORTED_PLATFORMS = {
    'linux': 'linux',
    'darwin': 'darwin',
    'win32': 'win'
};

const SUPPORTED_ARCHS = {
    'x64': 'x64',
    'arm64': 'arm64'
};

if (platform === 'linux' && arch === 'arm64') {
    // Explicitly support linux-arm64
} else if (!SUPPORTED_PLATFORMS[platform] || !SUPPORTED_ARCHS[arch]) {
    console.error(`Unsupported platform or architecture: ${platform}-${arch}`);
    process.exit(1);
}

const binaryName = 'lancedb-mcp-server';
const extension = platform === 'win32' ? '.exe' : '';
const assetName = `${binaryName}-${SUPPORTED_PLATFORMS[platform]}-${SUPPORTED_ARCHS[arch]}${extension}`;

// Assuming tag format is vX.Y.Z matches package.json version
const version = `v${packageJson.version}`;
const repoUrl = 'https://github.com/gongxh13/lancedb-mcp-server'; 
const downloadUrl = `${repoUrl}/releases/download/${version}/${assetName}`;

const binDir = path.join(__dirname, '../bin');
const outputPath = path.join(binDir, binaryName + extension);

console.log(`Downloading ${assetName} from ${downloadUrl}...`);

if (!fs.existsSync(binDir)) {
    fs.mkdirSync(binDir, { recursive: true });
}

const file = fs.createWriteStream(outputPath);

https.get(downloadUrl, (response) => {
    if (response.statusCode === 302 || response.statusCode === 301) {
        // Handle redirect
        https.get(response.headers.location, (response) => {
            if (response.statusCode !== 200) {
                console.error(`Failed to download binary. Status code: ${response.statusCode}`);
                process.exit(1);
            }
            response.pipe(file);
            file.on('finish', () => {
                file.close(() => {
                    console.log('Download completed.');
                    if (platform !== 'win32') {
                        fs.chmodSync(outputPath, '755');
                    }
                });
            });
        });
    } else if (response.statusCode !== 200) {
        console.error(`Failed to download binary. Status code: ${response.statusCode}`);
        process.exit(1);
    } else {
        response.pipe(file);
        file.on('finish', () => {
            file.close(() => {
                console.log('Download completed.');
                if (platform !== 'win32') {
                    fs.chmodSync(outputPath, '755');
                }
            });
        });
    }
}).on('error', (err) => {
    fs.unlink(outputPath, () => {}); // Delete the file async. (But we don't check result)
    console.error(`Error downloading binary: ${err.message}`);
    process.exit(1);
});
