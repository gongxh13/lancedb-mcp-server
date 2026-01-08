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
const tempOutputPath = outputPath + '.tmp';

console.log(`Downloading ${assetName} from ${downloadUrl}...`);

if (!fs.existsSync(binDir)) {
    fs.mkdirSync(binDir, { recursive: true });
}

let file = null;

function handleDownload(response) {
    if (response.statusCode !== 200) {
        console.error(`Failed to download binary. Status code: ${response.statusCode}`);
        process.exit(1);
    }

    file = fs.createWriteStream(tempOutputPath);

    const totalBytes = parseInt(response.headers['content-length'], 10);
    let downloadedBytes = 0;
    const progressBarWidth = 28;

    if (isNaN(totalBytes)) {
        console.log('Downloading (size unknown)...');
    }

    response.on('data', (chunk) => {
        downloadedBytes += chunk.length;
        if (!isNaN(totalBytes)) {
            const percent = ((downloadedBytes / totalBytes) * 100).toFixed(1);
            const filled = Math.max(
                0,
                Math.min(progressBarWidth, Math.round((downloadedBytes / totalBytes) * progressBarWidth))
            );
            const bar = `[${'#'.repeat(filled)}${'-'.repeat(progressBarWidth - filled)}]`;
            process.stdout.write(`\r${bar} ${percent}%`);
        }
    });

    response.pipe(file);

    file.on('finish', () => {
        if (!isNaN(totalBytes)) {
            process.stdout.write('\n');
        }
        file.close(() => {
            console.log('Download completed.');
            try {
                fs.renameSync(tempOutputPath, outputPath);
                if (platform !== 'win32') {
                    fs.chmodSync(outputPath, '755');
                }
            } catch (e) {
                console.error('Failed to move binary to final location:', e);
                process.exit(1);
            }
        });
    });
}

function onError(err) {
    if (file) {
        file.close();
        file.destroy();
    }
    try {
        if (fs.existsSync(tempOutputPath)) {
            fs.unlinkSync(tempOutputPath);
        }
    } catch (e) {
        // ignore
    }
    console.error(`Error downloading binary: ${err.message}`);
    process.exit(1);
}

https.get(downloadUrl, (response) => {
    if (response.statusCode === 302 || response.statusCode === 301) {
        https.get(response.headers.location, handleDownload).on('error', onError);
    } else {
        handleDownload(response);
    }
}).on('error', onError);
