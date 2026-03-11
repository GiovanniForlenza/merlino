# Merlino

Merlino is a macOS menu bar app that keeps websites always on top while you work.

Add any URL (Notion, Gmail, Linear, ChatGPT…) and open it with a click from the tray: the window stays above all other applications so you never lose track of it.

## Features

- Web windows always on top
- Open and close web apps with one click from the menu bar
- Added apps are remembered across restarts
- No Dock icon — lives only in the menu bar

## Requirements

- macOS 11 (Big Sur) or later
- Apple Silicon

## Installation

```bash
curl -fsSL https://raw.githubusercontent.com/giovanniforlenza/merlino/main/install.sh | bash
```

Once installed, find Merlino in `/Applications`. Launch it from Spotlight (`⌘ Space → Merlino`) or from the terminal:

```bash
open /Applications/Merlino.app
```

> On first launch macOS may show an "unidentified developer" warning. Go to **Settings → Privacy & Security** and click **Open Anyway**.

## Usage

1. Click the icon in the menu bar
2. Select **Add web app…**
3. Enter the name and URL of the site
4. The window opens and stays always on top

To remove a web app: open **Remove** in the menu and select the one to delete.

## License

MIT
