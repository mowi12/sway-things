# Wallpapers

Photos that Sway Things offers as wallpapers, taken by the people behind it and by photographer friends. The app
downloads them from here when someone picks one; they are not part of the app's packages or release tarball.

## Photographers

- **Niko Burkert**: `niko-burkert/`
- **Christoph Schmid**: `christoph-schmid/`

## License

The photos are **not** under the GPL like the rest of Sway Things. Their photographers keep all rights. Anyone using
Sway Things may use them as the wallpaper of their own desktop; nothing else is allowed: no sale, no use anywhere else,
no redistribution outside Sway Things, no commercial use. The full terms are in
[LICENSES/LicenseRef-Sway-Things-Wallpapers.txt](../LICENSES/LicenseRef-Sway-Things-Wallpapers.txt).

Each photo carries its photographer, copyright and these terms in its own metadata (EXIF, IPTC and XMP), so they stay
with the file.

## Adding a photo

- Only with the photographer's permission for these terms.
- Put it into the photographer's directory (`first-last/`), named after what it shows in lower case with hyphens.
- Scale it down to at most 3840 pixels on its longer side (never up), save it as JPEG with quality 90, and strip its
  metadata: camera serial numbers and GPS positions don't belong in a public repository. Then write the photographer,
  copyright, title and terms into it, as the existing photos have them (`exiftool -a -G1 <photo>` shows them).
- Add it to `wallpapers.toml` with its size, bytes and SHA-256, and the photographer's copyright to `REUSE.toml` if it
  is a new year or a new photographer.
