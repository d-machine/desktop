# -*- mode: python ; coding: utf-8 -*-
# PyInstaller spec for the Arthdesk FastAPI backend sidecar.
# Output: dist/backend/backend.exe  (--onedir, fastest startup)
# Then copy to: src-tauri/binaries/backend-x86_64-pc-windows-msvc.exe

block_cipher = None

a = Analysis(
    ["main.py"],
    pathex=["."],
    binaries=[],
    datas=[],
    hiddenimports=[
        # FastAPI / uvicorn internals
        "uvicorn.logging",
        "uvicorn.loops",
        "uvicorn.loops.auto",
        "uvicorn.protocols",
        "uvicorn.protocols.http",
        "uvicorn.protocols.http.auto",
        "uvicorn.protocols.websockets",
        "uvicorn.protocols.websockets.auto",
        "uvicorn.lifespan",
        "uvicorn.lifespan.on",
        # Cryptography
        "cryptography.hazmat.primitives.kdf.argon2",
        "cryptography.hazmat.primitives.ciphers.aead",
        "cryptography.hazmat.backends.openssl.backend",
        # PDF / XLSX
        "pdfplumber",
        "pdfminer",
        "pdfminer.high_level",
        "pypdf",
        "openpyxl",
        "xlsxwriter",
        # httpx
        "httpx",
        "httpcore",
        # App modules
        "core.auth",
        "core.db",
        "core.migrations",
        "core.session",
        "routers.auth",
        "routers.persons",
        "routers.portfolios",
        "routers.accounts",
        "routers.instruments",
        "routers.transactions",
        "routers.holdings",
        "routers.prices",
        "routers.reports",
        "routers.charges",
        "routers.tax",
        "routers.import_",
        "routers.backup",
        "routers.settings",
        "routers.deps",
        "services.holdings",
        "services.reports",
        "services.flags",
        "services.prices",
        "importers.common",
        "importers.pdf_utils",
        "pydantic_core._pydantic_core",
        "importers.angel_one",
        "importers.cams_cas",
        "importers.choice_mf",
        "importers.ce_global",
        "importers.icici_equity",
        "importers.cn_choice_equity",
        "importers.cn_woodstock",
        "importers.cn_nirmal_bang",
    ],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        "tkinter", "matplotlib", "numpy", "scipy", "PIL",
        "PyQt5", "PyQt6", "wx", "gi",
    ],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,    # --onedir: faster startup than --onefile
    name="backend",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,                # UPX can break some crypto libs
    console=True,             # keep console for log output (hidden in prod)
    disable_windowed_traceback=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.zipfiles,
    a.datas,
    strip=False,
    upx=False,
    upx_exclude=[],
    name="backend",
)
