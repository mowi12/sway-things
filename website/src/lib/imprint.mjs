// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// The imprint isn't in this repository: the operator's name and address stay on the operator's own site. The build of
// sway-things.org sets IMPRINT_URL (a repository variable) and fetches that page once per build; the deploy also sets
// IMPRINT_REQUIRED, so it fails rather than publish the site without an imprint. Other builds (pull requests, forks,
// local ones) show a note instead. The fetched markup is cut down to a few tags without attributes.

const allowedTags = new Set(['p', 'br', 'strong', 'b', 'em', 'i', 'h2', 'h3', 'ul', 'li']);
// Elements dropped with everything in them.
const droppedElements = new Set([
  'script',
  'style',
  'iframe',
  'object',
  'template',
  'svg',
  'math',
  'textarea',
  'title',
]);

/** Escapes text for HTML; an entity that is already there stays one. */
function escapeText(text) {
  return text
    .replace(/&(?!(?:[a-z]+|#\d+|#x[\da-f]+);)/gi, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

/**
 * Splits HTML into text and tags, in one pass from left to right, so nothing that is removed can bring a new tag
 * together: `{ text }`, or `{ name, closing, mailto }` for a tag (its name in lower case; `mailto` for a link to an
 * email address). Comments, declarations and the dropped elements with their content leave nothing; a `<` that starts
 * no tag is text.
 */
export function tokenize(html) {
  const tokens = [];
  let at = 0;
  let text = '';
  const flush = () => {
    if (text) {
      tokens.push({ text });
      text = '';
    }
  };
  while (at < html.length) {
    const open = html.indexOf('<', at);
    if (open === -1) {
      text += html.slice(at);
      break;
    }
    text += html.slice(at, open);
    if (html.startsWith('<!--', open)) {
      const end = html.indexOf('-->', open + 4);
      at = end === -1 ? html.length : end + 3;
      continue;
    }
    if (html[open + 1] === '!' || html[open + 1] === '?') {
      const end = html.indexOf('>', open);
      at = end === -1 ? html.length : end + 1;
      continue;
    }
    const tag = /^<(\/?)([a-z][a-z0-9]*)\b[^>]*>/i.exec(html.slice(open, open + 4096));
    if (!tag) {
      text += '<';
      at = open + 1;
      continue;
    }
    at = open + tag[0].length;
    const name = tag[2].toLowerCase();
    const closing = tag[1] === '/';
    if (!closing && droppedElements.has(name)) {
      // Skip to its end tag, or to the end if there is none.
      const end = html.toLowerCase().indexOf(`</${name}`, at);
      const close = end === -1 ? -1 : html.indexOf('>', end);
      at = close === -1 ? html.length : close + 1;
      continue;
    }
    flush();
    tokens.push({ name, closing, mailto: name === 'a' && /\bhref\s*=\s*["']?\s*mailto:/i.test(tag[0]) });
  }
  flush();
  return tokens;
}

/**
 * Drops a paragraph that is only one link (but for an email link): on the operator's site it leads to their other
 * pages (their own privacy notice, say), which mean nothing here.
 */
function withoutLinkParagraphs(tokens) {
  const blank = (token) => token?.text !== undefined && token.text.trim() === '';
  const kept = [];
  for (let at = 0; at < tokens.length; at++) {
    const token = tokens[at];
    if (token.name === 'p' && !token.closing) {
      let next = at + 1;
      while (blank(tokens[next])) next++;
      const link = tokens[next];
      if (link?.name === 'a' && !link.closing && !link.mailto) {
        let end = next + 1;
        while (tokens[end] && !(tokens[end].name === 'a' || tokens[end].name === 'p')) end++;
        if (tokens[end]?.name === 'a' && tokens[end].closing) {
          let after = end + 1;
          while (blank(tokens[after])) after++;
          if (tokens[after]?.name === 'p' && tokens[after].closing) {
            at = after;
            continue;
          }
        }
      }
    }
    kept.push(token);
  }
  return kept;
}

// Elements that hold blocks: a paragraph or a heading can't be inside one of these but a list item.
const headings = new Set(['h2', 'h3']);
const inline = new Set(['strong', 'b', 'em', 'i']);

/**
 * HTML from tokens: the allowed tags without attributes, and escaped text, always well formed. Like a browser, an
 * element that can't be inside the open ones closes them first (a paragraph or a heading closes an open paragraph, a
 * heading or an inline element; a list item closes the list item before it), a list item outside a list starts one, a
 * closing tag of an element that isn't open is dropped, and what is still open at the end is closed. So nothing of the
 * operator's markup spills into the page around it.
 */
function render(tokens) {
  const open = [];
  let html = '';
  const close = () => {
    html += `</${open.pop()}>`;
  };
  const closeTo = (name) => {
    while (open.includes(name)) {
      close();
    }
  };
  const top = () => open[open.length - 1];
  for (const token of tokens) {
    if (token.text !== undefined) {
      html += escapeText(token.text);
      continue;
    }
    const name = token.name;
    if (!allowedTags.has(name)) {
      continue;
    }
    if (name === 'br') {
      html += token.closing ? '' : '<br>';
      continue;
    }
    if (token.closing) {
      if (open.includes(name)) {
        closeTo(name);
      }
      continue;
    }
    if (name === 'p' || headings.has(name) || name === 'ul') {
      // Out of an open paragraph, heading or inline element: none of them holds a block.
      while (open.length > 0 && (top() === 'p' || headings.has(top()) || inline.has(top()))) {
        close();
      }
    } else if (name === 'li') {
      while (open.length > 0 && top() !== 'ul' && top() !== 'li') {
        close();
      }
      if (top() === 'li') {
        close();
      }
      if (top() !== 'ul') {
        html += '<ul>';
        open.push('ul');
      }
    }
    html += `<${name}>`;
    open.push(name);
  }
  while (open.length > 0) {
    close();
  }
  return html.replace(/\n\s+/g, '\n').trim();
}

/** Keeps only the allowed tags, drops every attribute and escapes everything else. */
export function sanitize(html) {
  return render(tokenize(html));
}

// The frame of the operator's site around the imprint, dropped with everything in it: its navigation, headers,
// footers and sidebars hold links and text that mean nothing here.
const frameElements = new Set(['nav', 'header', 'footer', 'aside']);
// The elements that hold a page's main content.
const contentElements = new Set(['main', 'article']);

/** Drops the frame elements (nested ones too) with everything in them. */
function withoutFrame(tokens) {
  const kept = [];
  let inside = null;
  let depth = 0;
  for (const token of tokens) {
    if (inside) {
      if (token.name === inside) {
        depth += token.closing ? -1 : 1;
      }
      if (depth === 0) {
        inside = null;
      }
      continue;
    }
    if (frameElements.has(token.name) && !token.closing) {
      inside = token.name;
      depth = 1;
      continue;
    }
    kept.push(token);
  }
  return kept;
}

/**
 * Where the imprint ends, for a heading from `start` to `end`: at the end of the innermost `main` or `article` the
 * heading is in, or else at the end of the body (or of the page).
 */
function contentEnd(tokens, start, end) {
  const open = [];
  for (const token of tokens.slice(0, start)) {
    if (!contentElements.has(token.name)) {
      continue;
    }
    if (!token.closing) {
      open.push(token.name);
    } else if (open.includes(token.name)) {
      open.length = open.lastIndexOf(token.name);
    }
  }
  const container = open[open.length - 1] ?? 'body';
  let depth = 0;
  for (let at = end + 1; at < tokens.length; at++) {
    const token = tokens[at];
    if (token.name !== container) {
      continue;
    }
    if (!token.closing) {
      depth++;
    } else if (depth === 0) {
      return at;
    } else {
      depth--;
    }
  }
  return tokens.length;
}

/** The length of the text in tokens, without tags and surrounding space. */
function textLength(tokens) {
  return tokens
    .filter((token) => token.text !== undefined)
    .map((token) => token.text)
    .join('')
    .trim().length;
}

/**
 * The imprint's content in a fetched page: everything after its first heading, up to the end of the `main` or
 * `article` the heading is in (else of the body), without the site's navigation, headers, footers and sidebars. The
 * heading
 * must say Impressum or Imprint, and there must be some text, so that a maintenance or error page answered with 200 OK
 * can't take the imprint's place.
 */
export function extract(page) {
  const tokens = tokenize(page);
  const start = tokens.findIndex((token) => token.name === 'h1' && !token.closing);
  const end = tokens.findIndex((token, at) => at > start && token.name === 'h1' && token.closing);
  if (start === -1 || end === -1) {
    throw new Error('the imprint page has no <h1> heading');
  }
  const heading = tokens
    .slice(start + 1, end)
    .map((token) => token.text ?? '')
    .join('')
    .trim();
  if (!/\b(impressum|imprint)\b/i.test(heading)) {
    throw new Error(`the imprint page's heading is "${heading}", not Impressum or Imprint`);
  }
  const content = withoutLinkParagraphs(withoutFrame(tokens.slice(end + 1, contentEnd(tokens, start, end))));
  if (textLength(content) < 80) {
    throw new Error('the imprint page looks empty');
  }
  return render(content);
}

/**
 * Fetches the imprint from IMPRINT_URL: `{ html, source }`, or null when no URL is set (and IMPRINT_REQUIRED isn't).
 * Throws when it can't be fetched or looks empty, so the build fails instead of publishing a site without one.
 */
export async function loadImprint(env = process.env) {
  const source = env.IMPRINT_URL?.trim();
  if (!source) {
    if (env.IMPRINT_REQUIRED) {
      throw new Error('IMPRINT_REQUIRED is set, but IMPRINT_URL is empty: set the repository variable IMPRINT_URL');
    }
    return null;
  }
  const url = new URL(source);
  if (url.protocol !== 'https:') {
    throw new Error(`IMPRINT_URL must use https: ${url}`);
  }
  const response = await fetch(url, { signal: AbortSignal.timeout(15_000), redirect: 'error' });
  if (!response.ok) {
    throw new Error(`the imprint could not be fetched from ${url} (${response.status})`);
  }
  try {
    return { html: extract(await response.text()), source: url.toString() };
  } catch (error) {
    throw new Error(`${error.message} (${url}); refusing to publish without an imprint`);
  }
}
