# macOS Code Signing Setup

This document explains how to set up code signing for macOS binaries in GitHub Actions.

## Required GitHub Secrets

You need to add the following secrets to your GitHub repository:

### 1. `APPLE_CERTIFICATE`
Your Apple Developer ID Application certificate in base64 format.

To export your certificate:
1. Open Keychain Access on macOS
2. Find your "Developer ID Application" certificate
3. Right-click and select "Export..."
4. Save as a .p12 file with a password
5. Convert to base64: `base64 -i certificate.p12 | pbcopy`
6. Add the base64 string as a GitHub secret

### 2. `APPLE_CERTIFICATE_PASSWORD`
The password you used when exporting the .p12 certificate.

### 3. `KEYCHAIN_PASSWORD`
A password for the temporary keychain created during CI. Can be any secure string.

### 4. `APPLE_SIGNING_IDENTITY`
Your signing identity, typically in the format: "Developer ID Application: Your Name (TEAMID)"

To find your signing identity:
```bash
security find-identity -v -p codesigning
```

### 5. `APPLE_ID`
Your Apple ID email address used for developer account.

### 6. `APPLE_PASSWORD`
An app-specific password for notarization.

To create an app-specific password:
1. Go to https://appleid.apple.com/account/manage
2. Sign in with your Apple ID
3. In the "Sign-In and Security" section, select "App-Specific Passwords"
4. Click the plus button to generate a new password
5. Name it something like "GitHub Actions Notarization"
6. Copy the generated password

### 7. `APPLE_TEAM_ID`
Your Apple Developer Team ID (10-character string).

You can find this in:
- Apple Developer portal under Membership
- Or in your signing identity (the part in parentheses)

## Adding Secrets to GitHub

1. Go to your repository on GitHub
2. Click Settings → Secrets and variables → Actions
3. Click "New repository secret" for each secret above
4. Enter the name and value for each secret

## Testing

After setting up the secrets, your CI will:
1. Import the certificate into a temporary keychain
2. Sign the app during the Tauri build process
3. Notarize the DMG file with Apple
4. Staple the notarization ticket to the DMG

The signed and notarized app will run on any macOS system without security warnings.