// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// Reads the parts of README.md the site shows, so the site and the README can't disagree. Every function throws when
// the README no longer has the shape it expects: a changed README fails the build instead of a silently wrong page.

import { createMarkdownProcessor } from '@astrojs/markdown-remark';

import readme from '../../../README.md?raw';

const processor = await createMarkdownProcessor({ syntaxHighlight: false, smartypants: false });

/** The Markdown below the heading `## title`, up to the next `## ` heading. */
function section(title) {
  const lines = readme.split('\n');
  const start = lines.indexOf(`## ${title}`);
  if (start === -1) {
    throw new Error(`README.md has no section "## ${title}"`);
  }
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((line) => line.startsWith('## '));
  return (end === -1 ? rest : rest.slice(0, end)).join('\n').trim();
}

/** Splits Markdown at its `### ` headings: the text before the first one and every subsection. */
function subsections(markdown) {
  const [intro, ...parts] = markdown.split(/^### /m);
  return {
    intro: intro.trim(),
    parts: parts.map((part) => {
      const newline = part.indexOf('\n');
      return { title: part.slice(0, newline).trim(), body: part.slice(newline + 1).trim() };
    }),
  };
}

async function html(markdown) {
  return (await processor.render(markdown)).code;
}

function slug(text) {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
}

/** The principles: `- **Title.** Text` items. */
export async function principles() {
  const items = section('Principles').split(/^- /m).slice(1);
  if (items.length === 0) {
    throw new Error('README.md: "## Principles" has no list items');
  }
  return Promise.all(
    items.map(async (item) => {
      const match = /^\*\*(.+?)\*\*\s+([\s\S]+)$/.exec(item.trim());
      if (!match) {
        throw new Error(`README.md: a principle doesn't start with a bold title: ${item.slice(0, 40)}`);
      }
      return { title: match[1].replace(/\.$/, ''), html: await html(match[2].replace(/\s+/g, ' ')) };
    }),
  );
}

/** The sidebar groups of "## Features" and the page names in each, from the bold names of their list items. */
export function featureGroups() {
  const { parts } = subsections(section('Features'));
  if (parts.length === 0) {
    throw new Error('README.md: "## Features" has no groups');
  }
  return parts.map(({ title, body }) => {
    const pages = [...body.matchAll(/\*\*(.+?)\*\*/g)].map((match) => match[1].replace(/:$/, ''));
    if (pages.length === 0) {
      throw new Error(`README.md: the feature group "${title}" names no pages`);
    }
    return { title, pages };
  });
}

/** The example commands of "## Also a command line tool". */
export function cliExample() {
  const match = /```sh\n([\s\S]+?)```/.exec(section('Also a command line tool'));
  if (!match) {
    throw new Error('README.md: "## Also a command line tool" has no sh code block');
  }
  return match[1].trimEnd();
}

/** The requirements, as HTML. */
export async function requirements() {
  return html(section('Requirements'));
}

/**
 * The installation section: its introduction and one entry per distribution, as HTML. An entry keeps the text before
 * its commands and the commands, up to the end of the last code block; the notes after them stay in the README.
 */
export async function installation() {
  const { intro, parts } = subsections(section('Installation'));
  if (parts.length === 0) {
    throw new Error('README.md: "## Installation" has no subsections');
  }
  return {
    intro: await html(intro),
    methods: await Promise.all(
      parts.map(async ({ title, body }) => {
        const end = body.lastIndexOf('\n```');
        if (end === -1 || !/^```/m.test(body)) {
          throw new Error(`README.md: the installation method "${title}" has no code block`);
        }
        return { id: slug(title), title, html: await html(body.slice(0, end + 4)) };
      }),
    ),
  };
}
