// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// Run by scripts/check-website.sh: node --test website/src/lib/*.test.mjs

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { extract, loadImprint, sanitize } from './imprint.mjs';

const page = `<!DOCTYPE html><html><head><title>Impressum</title><style>p { color: red }</style></head>
<body><div class="page"><a href="/">zurück</a><h1>Impressum</h1>
<p><strong>Angaben gemäß § 5 DDG:</strong></p>
<p class="x" onclick="alert(1)">Erika Mustermann<br>Musterstraße 1<br>12345 Musterstadt</p>
<script>alert(1)</script><img src=x onerror=alert(1)><a href="javascript:alert(1)">Link</a>
<h2>Haftung</h2><p>Text &amp; mehr, 1 < 2</p>
<p><a href="/datenschutz">Datenschutzerklärung</a></p>
<p><a href="mailto:erika@example.org">erika@example.org</a></p>
<h1>Kontakt</h1><p>Weiteres nach einer zweiten Überschrift</p></div></body></html>`;

test('keeps the text and the allowed tags without attributes', () => {
  const html = extract(page);
  assert.ok(html.startsWith('<p><strong>Angaben gemäß § 5 DDG:</strong></p>'));
  assert.ok(html.includes('<p>Erika Mustermann<br>Musterstraße 1<br>12345 Musterstadt</p>'));
  assert.ok(html.includes('<h2>Haftung</h2><p>Text &amp; mehr, 1 &lt; 2</p>'));
});

test('drops scripts, styles, images, links, attributes and what comes before the heading', () => {
  const html = extract(page);
  for (const unwanted of ['script', 'alert', 'onclick', 'onerror', 'img', 'href', 'class', 'zurück', 'color: red']) {
    assert.ok(!html.includes(unwanted), `${unwanted} in ${html}`);
  }
  // The text of a link stays, without the link; a paragraph that is only a link goes.
  assert.ok(html.includes('Link'));
  assert.ok(!html.includes('Datenschutzerklärung'), html);
  // An email link keeps its address, and what follows a second heading stays.
  assert.ok(html.includes('<p>erika@example.org</p>'), html);
  assert.ok(html.includes('Weiteres nach einer zweiten'), html);
});

test('sanitize removes comments and unknown tags', () => {
  assert.equal(sanitize('<p>a<!-- b --><span style="x">c</span></p><svg><script>x</script></svg>'), '<p>ac</p>');
});

test('a page without the imprint heading or with too little text is refused', () => {
  assert.throws(() => extract('<p>no heading</p>'), /no <h1>/);
  assert.throws(() => extract('<h1>Impressum</h1><p>tbd</p>'), /looks empty/);
  const maintenance = '<h1>Maintenance</h1><p>This site is down for maintenance right now. Please try again later;'
    + ' we are working on it and will be back soon.</p>';
  assert.throws(() => extract(maintenance), /not Impressum or Imprint/);
});

test('without a URL there is no imprint, unless one is required', async () => {
  assert.equal(await loadImprint({}), null);
  await assert.rejects(loadImprint({ IMPRINT_REQUIRED: '1' }), /IMPRINT_URL is empty/);
  await assert.rejects(loadImprint({ IMPRINT_URL: 'http://example.org/impressum' }), /must use https/);
});

test('no tag but the allowed ones survives, however the markup is nested or broken', () => {
  const hostile = [
    '<scr<script>ipt>alert(1)</script>',
    '<<script>script>alert(1)<</script>/script>',
    '<!--<script>-->alert(1)<!-- -->',
    '<!-<!-- x -->-><script>alert(1)</script>',
    '<p onclick="x">a</p><svg><script>alert(1)</script></svg><img src=x onerror=alert(1)>',
    '<script>never closed',
    '<style>p{}</style><textarea><p>x</textarea>',
    '<a href="javascript:alert(1)">x</a><iframe src="x"></iframe>',
    '<p>1 < 2 > 0 & &amp; &lt;script&gt;</p>',
    '<!DOCTYPE html><?xml x?><p/>',
  ];
  const random = Array.from({ length: 2000 }, (_, seed) => {
    const parts = ['<', '>', '/', '!', '-', 'script', 'p', 'a ', '"', '=', 'x', '<!--', '-->', ' ', 'img', 'on'];
    let state = seed + 1;
    let text = '';
    for (let n = 0; n < 24; n++) {
      state = (state * 1103515245 + 12345) % 2147483648;
      text += parts[state % parts.length];
    }
    return text;
  });
  for (const input of [...hostile, ...random]) {
    const html = sanitize(input);
    for (const tag of html.matchAll(/<\/?([^\s>]*)[^>]*>/g)) {
      assert.match(tag[0], /^<\/?(p|br|strong|b|em|i|h2|h3|ul|li)>$/, `${input} gave ${html}`);
    }
    assert.ok(!html.includes('<script') && !html.includes('<!--'), `${input} gave ${html}`);
  }
});

test('the markup is always well formed, however broken the page is', () => {
  // Unclosed list, item and heading: closed at the end (a heading may stand in a list item).
  assert.equal(sanitize('<ul><li>one<li>two<h2>Title'), '<ul><li>one</li><li>two<h2>Title</h2></li></ul>');
  // A closing tag with no opening one goes; an inline element left open closes with its paragraph.
  assert.equal(sanitize('</p></ul>text<p><strong>bold<p>next'), 'text<p><strong>bold</strong></p><p>next</p>');
  // A list item outside a list starts one; a paragraph or a heading closes an open paragraph.
  assert.equal(sanitize('<li>alone</li>'), '<ul><li>alone</li></ul>');
  assert.equal(sanitize('<p>one<h3>head</h3>'), '<p>one</p><h3>head</h3>');
  // Crossed tags are closed in order, never left crossed.
  assert.equal(sanitize('<p><em>a<strong>b</em>c</strong></p>'), '<p><em>a<strong>b</strong></em>c</p>');
});

test("the operator site's navigation, header, footer and sidebar don't come along", () => {
  const framed = `<html><body><header><nav><a href="/">Home</a></nav><p>Erika's site</p></header>
<main><article><h1>Impressum</h1>
<p>Erika Mustermann<br>Musterstraße 1<br>12345 Musterstadt, reachable at the address below.</p>
<aside><h2>Mehr</h2><p>Seitenleiste</p></aside>
<section><h2>Haftung</h2><p>Text</p></section>
<nav><ul><li><a href="/blog">Blog</a></li><li>Projekte</li></ul></nav>
</article><article><p>Another article</p></article></main>
<footer><p>© Erika</p><ul><li>Fußzeile</li></ul></footer></body></html>`;
  const html = extract(framed);
  assert.ok(html.includes('<p>Erika Mustermann<br>'));
  assert.ok(html.includes('<h2>Haftung</h2><p>Text</p>'));
  for (const unwanted of [
    "Erika's site",
    'Seitenleiste',
    'Mehr',
    'Blog',
    'Projekte',
    'Another article',
    '© Erika',
    'Fußzeile',
  ]) {
    assert.ok(!html.includes(unwanted), `${unwanted} is in ${html}`);
  }
});

test('the heading in a header still starts the imprint, which ends with the main content', () => {
  const html = extract(`<body><main><header><h1>Imprint</h1></header>
<p>Erika Mustermann, Musterstraße 1, 12345 Musterstadt, Germany, reachable at the address below.</p></main>
<p>After the main content</p></body>`);
  assert.ok(html.startsWith('<p>Erika Mustermann'));
  assert.ok(!html.includes('After the main content'));
});
