const { spawnSync } = require("child_process");
const { platform, arch } = process;

// Map Node.js platform/arch to the package names.
const PLATFORMS = {
  "darwin-arm64": "@asterai-io/cli-darwin-arm64",
  "darwin-x64": "@asterai-io/cli-darwin-x64",
  "linux-arm64": "@asterai-io/cli-linux-arm64",
  "linux-x64": "@asterai-io/cli-linux-x64",
  "win32-x64": "@asterai-io/cli-win32-x64",
};

function getBinaryPath() {
  const key = `${platform}-${arch}`;
  const packageName = PLATFORMS[key];

  if (!packageName) {
    console.error(
      `Unsupported platform: ${platform}-${arch}\n` +
        `Supported platforms: ${Object.keys(PLATFORMS).join(", ")}`
    );
    process.exit(1);
  }

  try {
    const binName = platform === "win32" ? "asterai.exe" : "asterai";
    return require.resolve(`${packageName}/bin/${binName}`);
  } catch (e) {
    console.error(
      `Could not find asterai binary for ${platform}-${arch}.\n` +
        `Package ${packageName} may not be installed.\n` +
        `Try reinstalling: npm install -g @asterai/cli`
    );
    process.exit(1);
  }
}

const binaryPath = getBinaryPath();
const result = spawnSync(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  env: process.env,
});

if (result.error) {
  console.error("Failed to run asterai:", result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
