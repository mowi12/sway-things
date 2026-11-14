// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import { iconSvg } from '../lib/icon.mjs';

export function GET() {
  return new Response(iconSvg, { headers: { 'Content-Type': 'image/svg+xml' } });
}
