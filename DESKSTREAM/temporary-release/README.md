# DeskStream — Temporary Development Release

**Build Date:** 2026-10-13  
**Status:** UNTRUSTED DEVELOPMENT ARTIFACT (Not for general production distribution)  
**Target Environment:** Internal Testing & Development

---

## ⚠️ Important Security Notice: Windows Smart App Control & SmartScreen

1. **Signing Status: Unsigned (Development Mode)**
   - These binaries are **not digitally signed** with an EV or OV code-signing certificate trusted by the Microsoft Trusted Root Certificate Program.
   - Self-signing was evaluated and intentionally rejected for this distribution because self-signed certificates do not chain to a trusted public Certificate Authority and still trigger Windows Smart App Control / SmartScreen blocks on external machines.

2. **Windows SmartScreen / Smart App Control Behavior**
   - **Smart App Control (Windows 11):** Will block execution of unsigned and untrusted executables by default.
   - **Microsoft Defender SmartScreen:** Will show `"Windows protected your PC"` with `"Unknown publisher"`.
   - **DO NOT** attempt to bypass this by disabling Smart App Control or Defender, altering registry policies, or installing untrusted certificates into Windows Root Stores.
   - For internal developer testing on dedicated test machines, standard developer sideloading / testing environments or Windows Developer Mode should be utilized until a production certificate is procured.

3. **Production Signing Resolution in Progress**
   - A trusted production code-signing certificate (from DigiCert, Sectigo, or an authorized Microsoft Root CA) is being arranged.
   - Once acquired, the automated scripts `scripts\sign-release.ps1` and `desktop-agent\build-msix.ps1` will sign the production binaries with full trusted timestamping, enabling seamless SmartScreen reputation and Smart App Control compliance.

---

## Included Artifacts & SHA-256 Hashes

| File | Purpose | Signing Status | SHA-256 Hash |
| :--- | :--- | :--- | :--- |
| `DeskStream.exe` | Standalone Desktop Agent | Unsigned | `926D3FC7B3E880C4EAFB3769191ADA64CC46DEDA0B545AA7CD429DB80EDD72C4` |
| `DeskStream-Setup-x64-development.exe` | NSIS Windows Installer | Unsigned | `7DCCDDF7C8B25ACB015E157AE406407CF72EA4C4148BF607259AAF912FAF4638` |

---

## Verification Steps

To verify file integrity in PowerShell:

```powershell
Get-FileHash .\DeskStream.exe -Algorithm SHA256
Get-FileHash .\DeskStream-Setup-x64-development.exe -Algorithm SHA256
Get-AuthenticodeSignature .\DeskStream.exe
Get-AuthenticodeSignature .\DeskStream-Setup-x64-development.exe
```
