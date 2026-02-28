# Download python-build-standalone + install pip deps into src-tauri\python-runtime\
# Usage: pwsh scripts/setup-python.ps1
$ErrorActionPreference = "Stop"

$PYTHON_VERSION = "3.12.8"
$STANDALONE_TAG = "20250106"
$TARGET_DIR = "src-tauri\python-runtime"
$REQUIREMENTS = "src-tauri\requirements.txt"
$TRIPLE = "x86_64-pc-windows-msvc"
$PYTHON_BIN = "$TARGET_DIR\python.exe"

$FILENAME = "cpython-${PYTHON_VERSION}+${STANDALONE_TAG}-${TRIPLE}-install_only_stripped.tar.gz"
$URL = "https://github.com/astral-sh/python-build-standalone/releases/download/${STANDALONE_TAG}/${FILENAME}"

# ─── Skip if already set up ───────────────────────────────────────
if (Test-Path $PYTHON_BIN) {
    $existingVer = & $PYTHON_BIN --version 2>&1
    if ($existingVer -match $PYTHON_VERSION) {
        Write-Host "Python $PYTHON_VERSION already exists at $PYTHON_BIN, skipping download."
        Write-Host "To force re-download, delete $TARGET_DIR\ and re-run."
        Write-Host "Installing pip dependencies..."
        & $PYTHON_BIN -m pip install -r $REQUIREMENTS --no-cache-dir -q
        Write-Host "Done."
        exit 0
    }
}

# ─── Download ──────────────────────────────────────────────────────
Write-Host "Downloading Python $PYTHON_VERSION for $TRIPLE..."
Write-Host "URL: $URL"

$tmpDir = New-TemporaryFile | ForEach-Object { Remove-Item $_; New-Item -ItemType Directory -Path $_ }
$archive = Join-Path $tmpDir $FILENAME

try {
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $URL -OutFile $archive -UseBasicParsing
} catch {
    Write-Error "Download failed: $_"
    exit 1
}

# ─── Extract ───────────────────────────────────────────────────────
Write-Host "Extracting to $TARGET_DIR\..."
if (Test-Path $TARGET_DIR) {
    Remove-Item -Recurse -Force $TARGET_DIR
}

$parentDir = Split-Path $TARGET_DIR -Parent
# tar.exe is built into Windows 10+
tar xzf $archive -C $parentDir

# python-build-standalone archives contain a top-level `python\` directory
$extractedDir = Join-Path $parentDir "python"
Rename-Item $extractedDir $TARGET_DIR

Write-Host "Python binary: $PYTHON_BIN"
& $PYTHON_BIN --version

# ─── Install pip dependencies ──────────────────────────────────────
Write-Host "Installing pip dependencies from $REQUIREMENTS..."
& $PYTHON_BIN -m pip install -r $REQUIREMENTS --no-cache-dir -q

# ─── Slim down ─────────────────────────────────────────────────────
Write-Host "Removing unnecessary files to reduce bundle size..."

$libDir = Join-Path $TARGET_DIR "Lib"

# Remove test directories
Get-ChildItem -Path $TARGET_DIR -Recurse -Directory -Filter "test" -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
Get-ChildItem -Path $TARGET_DIR -Recurse -Directory -Filter "tests" -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

# Remove __pycache__
Get-ChildItem -Path $TARGET_DIR -Recurse -Directory -Filter "__pycache__" -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

# Remove .pyc files
Get-ChildItem -Path $TARGET_DIR -Recurse -Filter "*.pyc" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue

# Remove unused stdlib modules
foreach ($dir in @("tkinter", "idlelib", "turtle", "turtledemo", "ensurepip", "lib2to3", "distutils")) {
    $path = Join-Path $libDir $dir
    if (Test-Path $path) {
        Remove-Item -Recurse -Force $path -ErrorAction SilentlyContinue
    }
}

# Remove pip (not needed at runtime)
$pipDir = Join-Path $libDir "site-packages\pip"
if (Test-Path $pipDir) {
    Remove-Item -Recurse -Force $pipDir -ErrorAction SilentlyContinue
}

# Remove .dist-info directories
Get-ChildItem -Path $TARGET_DIR -Recurse -Directory -Filter "*.dist-info" -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

# ─── Cleanup temp files ───────────────────────────────────────────
Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue

# ─── Summary ───────────────────────────────────────────────────────
$size = (Get-ChildItem -Recurse $TARGET_DIR | Measure-Object -Property Length -Sum).Sum / 1MB
$sizeStr = "{0:N0} MB" -f $size

Write-Host ""
Write-Host "Setup complete!"
Write-Host "  Python: $PYTHON_BIN"
Write-Host "  Size:   $sizeStr"
& $PYTHON_BIN --version
