#!/bin/bash
set -e

REPO="giovanniforlenza/merlino"
APP_NAME="Merlino"
INSTALL_DIR="/Applications"

# Colori
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo ""
echo "  Installing ${APP_NAME}..."
echo ""

# Controlla macOS
if [[ "$(uname)" != "Darwin" ]]; then
  echo -e "${RED}Errore: questo script funziona solo su macOS.${NC}"
  exit 1
fi

# Recupera URL dell'ultimo .dmg da GitHub Releases
echo "  Cerco l'ultima versione..."
DMG_URL=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep "browser_download_url" \
  | grep "\.dmg" \
  | head -1 \
  | cut -d '"' -f 4)

if [[ -z "$DMG_URL" ]]; then
  echo -e "${RED}Errore: nessuna release trovata su GitHub.${NC}"
  echo "  Assicurati che esista almeno una release con un file .dmg allegato."
  exit 1
fi

VERSION=$(echo "$DMG_URL" | grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+' | head -1)
echo "  Versione trovata: ${VERSION}"

# Scarica il .dmg in una cartella temporanea
TMP_DIR=$(mktemp -d)
DMG_PATH="${TMP_DIR}/${APP_NAME}.dmg"

echo "  Download in corso..."
curl -fsSL --progress-bar "$DMG_URL" -o "$DMG_PATH"

# Monta il .dmg
echo "  Installazione..."
MOUNT_POINT=$(hdiutil attach "$DMG_PATH" -nobrowse -quiet | tail -1 | awk -F'\t' '{print $NF}')

# Copia l'app in /Applications (sovrascrive versione precedente)
if [[ -d "${INSTALL_DIR}/${APP_NAME}.app" ]]; then
  rm -rf "${INSTALL_DIR}/${APP_NAME}.app"
fi
cp -r "${MOUNT_POINT}/${APP_NAME}.app" "${INSTALL_DIR}/"

# Smonta il .dmg e pulisce
hdiutil detach "$MOUNT_POINT" -quiet
rm -rf "$TMP_DIR"

echo ""
echo -e "  ${GREEN}${APP_NAME} installato con successo in ${INSTALL_DIR}/${NC}"
echo ""
echo "  Puoi avviarlo da Spotlight (⌘+Space → Merlino)"
echo "  oppure da terminale:"
echo "    open /Applications/${APP_NAME}.app"
echo ""

# Rimuovi quarantena (evita il popup 'sviluppatore non identificato' al primo avvio)
xattr -dr com.apple.quarantine "${INSTALL_DIR}/${APP_NAME}.app" 2>/dev/null || true
