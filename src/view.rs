//! `recall view` — generate a readable, self-contained HTML page for one
//! session (turn anchors: `#turn-N`), optionally opening it in the browser.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Args;

use crate::db::{Db, SessionRow, TurnRow, default_db_path};
use crate::i18n::{Lang, current, tr, tr_in, trf, trf_in};
use crate::md;
use crate::util::fmt_ms_local;

#[derive(Args, Debug)]
pub struct ViewArgs {
    /// Session id or a unique prefix
    pub session: String,
    /// Anchor this turn (the page still contains every turn; --open jumps to it)
    #[arg(long)]
    pub turn: Option<i64>,
    /// Output path (default: <temp dir>/recall-view/<id>.html)
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Open the generated page in the default browser
    #[arg(long)]
    pub open: bool,
    /// Use the light theme (dark night theme is the default)
    #[arg(long)]
    pub light: bool,
    #[arg(long)]
    pub db: Option<PathBuf>,
}

pub fn run(args: ViewArgs) -> Result<()> {
    let db = Db::open(&args.db.clone().unwrap_or_else(default_db_path))?;
    let Some(row) = db.find_session(&args.session)? else {
        bail!("{}", trf("view.no_session", &[&args.session]));
    };
    let turns = db.turns_for(&row.id)?;

    let html = render_page(&row, &turns, args.turn, args.light, current());
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| default_view_path(&row.id));
    if let Some(dir) = out.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out, html)?;

    let url = file_url(&out);
    println!("{}", trf("view.generated", &[&out.display().to_string()]));
    let target = match args.turn {
        Some(t) => format!("{url}#turn-{t}"),
        None => url,
    };
    if args.open {
        open_in_browser(&target)?;
        println!("{}", tr("view.opened"));
    } else {
        println!("{}", trf("view.browser", &[&target]));
    }
    Ok(())
}

fn default_view_path(id: &str) -> PathBuf {
    // Keep generated pages out of the workspace (they would otherwise show up
    // as diagnostics noise); temp dir is per-user and disposable.
    std::env::temp_dir()
        .join("recall-view")
        .join(format!("{id}.html"))
}

fn file_url(path: &std::path::Path) -> String {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = abs.display().to_string().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s);
    format!("file:///{}", s.trim_start_matches('/'))
}

fn open_in_browser(url: &str) -> Result<()> {
    #[cfg(windows)]
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .status();
    #[cfg(not(windows))]
    let status = std::process::Command::new("xdg-open").arg(url).status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => bail!("{}", trf("view.browser_failed", &[&s.to_string()])),
        Err(e) => bail!("{}", trf("view.browser_missing", &[&e.to_string()])),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Keeps the left rail in sync with the turn currently being read: the active
/// entry is highlighted and smoothly scrolled to the vertical center of the
/// rail (re-centered after hover-expand and on click/navigation).
const TOC_SCRIPT: &str = r#"(function () {
  var header = document.querySelector('header.meta');
  function syncHeader() {
    if (header) {
      // Set on body: the theme rule also declares --header-h on body, which
      // would otherwise shadow a value set on the root element.
      document.body.style.setProperty('--header-h', header.offsetHeight + 'px');
    }
  }
  syncHeader();
  window.addEventListener('resize', syncHeader);
  setTimeout(syncHeader, 300);
  var nav = document.querySelector('.toc');
  var links = Array.prototype.slice.call(nav ? nav.querySelectorAll('a') : []);
  if (!links.length) return;
  var targets = links.map(function (a) {
    return document.getElementById(a.getAttribute('href').slice(1));
  });
  var current = 0;
  var raf = 0;
  var animRaf = 0;
  var lastTarget = null;
  var first = true;
  function animateTo(top) {
    if (first) {
      nav.scrollTop = top;
      lastTarget = top;
      return;
    }
    if (lastTarget !== null && Math.abs(top - lastTarget) < 1) return;
    lastTarget = top;
    cancelAnimationFrame(animRaf);
    var start = nav.scrollTop;
    var dist = top - start;
    if (Math.abs(dist) < 2) {
      nav.scrollTop = top;
      return;
    }
    var dur = Math.max(130, Math.min(340, 130 + Math.abs(dist) * 0.8));
    var t0 = performance.now();
    function step(now) {
      var p = Math.min(1, (now - t0) / dur);
      var eased = 1 - Math.pow(1 - p, 3);
      nav.scrollTop = start + dist * eased;
      if (p < 1) animRaf = requestAnimationFrame(step);
    }
    animRaf = requestAnimationFrame(step);
  }
  function centerActive() {
    var a = links[current];
    if (!a) return;
    var top = a.offsetTop - (nav.clientHeight - a.offsetHeight) / 2;
    animateTo(top);
  }
  function update() {
    raf = 0;
    var idx = 0;
    for (var i = 0; i < targets.length; i++) {
      var t = targets[i];
      if (!t) continue;
      if (t.getBoundingClientRect().top <= 120) idx = i; else break;
    }
    current = idx;
    for (var j = 0; j < links.length; j++) {
      links[j].classList.toggle('active', j === idx);
    }
    centerActive();
    first = false;
  }
  window.addEventListener('scroll', function () {
    if (!raf) raf = window.requestAnimationFrame(update);
  }, { passive: true });
  window.addEventListener('resize', function () {
    if (!raf) raf = window.requestAnimationFrame(update);
  });
  nav.addEventListener('mouseenter', centerActive);
  nav.addEventListener('transitionend', function (e) {
    if (e.target === nav) centerActive();
  });
  update();
})();"#;

/// Messages longer than this (chars) get a preview + disclosure.
const COLLAPSE_CHARS: usize = 1500;
/// How much text stays visible above the disclosure.
const PREVIEW_CHARS: usize = 700;

pub fn render_page(
    row: &SessionRow,
    turns: &[TurnRow],
    target_turn: Option<i64>,
    light: bool,
    lang: Lang,
) -> String {
    let title = row
        .title
        .clone()
        .unwrap_or_else(|| tr_in(lang, "common.no_title").to_string());
    let mut body = String::with_capacity(1 << 20);
    let badge = |idx: i64| trf_in(lang, "page.turn_badge", &[&idx.to_string()]);

    for t in turns {
        let rid = t
            .request_id
            .as_deref()
            .map(|r| format!("<span class=\"rid\">{}</span>", esc(r)))
            .unwrap_or_default();
        body.push_str(&format!(
            "<section class=\"turn\" id=\"turn-{idx}\">\
             <div class=\"turn-head\"><span class=\"badge\">{badge}</span>{rid}</div>",
            idx = t.idx,
            badge = badge(t.idx),
            rid = rid,
        ));
        let open = target_turn == Some(t.idx);
        push_message(
            &mut body,
            lang,
            "user",
            tr_in(lang, "page.user"),
            &t.user_text,
            open,
        );
        push_message(
            &mut body,
            lang,
            "assistant",
            tr_in(lang, "page.assistant"),
            &t.assistant_text,
            open,
        );
        body.push_str("</section>");
    }

    let max_turn = turns.iter().map(|t| t.idx).max().unwrap_or(0);
    let max_label = badge(max_turn);
    // Reserve the hover width up front (widest "turn N") so the rail does not
    // jitter as the pointer moves between entries.
    let toc_w = 24 + max_label.chars().count() * 7;
    let mut toc = String::with_capacity(turns.len() * 90);
    toc.push_str(&format!(
        "<nav class=\"toc\" style=\"--toc-w:{toc_w}px\" aria-label=\"{}\">",
        esc(tr_in(lang, "page.toc_label"))
    ));
    for t in turns {
        toc.push_str(&format!(
            "<a href=\"#turn-{idx}\" title=\"{badge}\"><span class=\"dash\">–</span>\
             <span class=\"lbl\">{badge}</span></a>",
            idx = t.idx,
            badge = badge(t.idx)
        ));
    }
    toc.push_str("</nav>");

    let meta1 = format!(
        "{} · {} · {} · <code>{}</code>",
        trf_in(lang, "page.project", &[&esc(&row.project_label)]),
        trf_in(lang, "page.created", &[&fmt_ms_local(row.created_ms)]),
        trf_in(lang, "page.turns", &[&turns.len().to_string()]),
        esc(&row.id)
    );
    let meta2 = trf_in(lang, "page.source", &[&esc(&row.source_path)]);
    let expand = tr_in(lang, "page.expand");
    let collapse = tr_in(lang, "page.collapse");
    let css_text = format!(
        "{CSS}\nsummary .lbl::after {{ content: \"{expand}\"; }}\n\
         details[open] summary .lbl::after {{ content: \"{collapse}\"; }}\n"
    );

    let theme = if light { "light" } else { "dark" };
    format!(
        "<!DOCTYPE html>\n<html lang=\"{lang_code}\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title} · recall</title>\n<style>{css}</style>\n</head>\n<body data-theme=\"{theme}\">\n\
         {toc}\n\
         <header class=\"meta\">\n<h1>{title}</h1>\n\
         <div class=\"sub\">{meta1}</div>\n\
         <div class=\"sub\">{meta2}</div>\n</header>\n\
         <main>\n{body}</main>\n<footer>{footer}</footer>\n\
         <script>{script}</script>\n</body>\n</html>\n",
        title = esc(&title),
        css = css_text,
        theme = theme,
        lang_code = lang.code(),
        toc = toc,
        script = TOC_SCRIPT,
        meta1 = meta1,
        meta2 = meta2,
        footer = esc(tr_in(lang, "page.footer")),
        body = body,
    )
}

/// One user/assistant message: prominent role pill, left accent bar. Long
/// messages get a rendered **preview** plus a `<details>` disclosure for the
/// remainder; when open, the "收起" summary is sticky (bounded by this
/// message's box) so it stays reachable while scrolling.
fn push_message(out: &mut String, lang: Lang, kind: &str, label: &str, text: &str, open: bool) {
    out.push_str(&format!(
        "<div class=\"msg msg-{kind}\"><div class=\"role\"><span class=\"dot\"></span>\
         <span class=\"name\">{label}</span></div><div class=\"body\">"
    ));
    if text.trim().is_empty() {
        out.push_str(&format!(
            "<p class=\"empty\">{}</p>",
            esc(tr_in(lang, "page.empty"))
        ));
    } else {
        let chars = text.chars().count();
        if chars > COLLAPSE_CHARS {
            let (preview, rest) = split_for_preview(text, PREVIEW_CHARS);
            out.push_str("<div class=\"md preview\">");
            out.push_str(&md::to_html(preview));
            out.push_str("</div>");
            out.push_str(&format!(
                "<details class=\"more\"{}><summary><span class=\"chev\"></span>\
                 <span class=\"lbl\"></span><span class=\"meta\">{}</span></summary><div class=\"md\">",
                if open { " open" } else { "" },
                esc(&trf_in(lang, "page.chars", &[&chars.to_string()])),
            ));
            out.push_str(&md::to_html(rest));
            out.push_str("</div></details>");
        } else {
            out.push_str("<div class=\"md\">");
            out.push_str(&md::to_html(text));
            out.push_str("</div>");
        }
    }
    out.push_str("</div></div>");
}

/// Split at the last blank line that is NOT inside a fenced code block and
/// lies within `max_chars`; falls back to a hard character cut. The returned
/// remainder is never empty for texts longer than `max_chars`.
fn split_for_preview(text: &str, max_chars: usize) -> (&str, &str) {
    if text.chars().count() <= max_chars {
        return (text, "");
    }
    let mut in_fence = false;
    let mut chars_before = 0usize;
    let mut best: Option<usize> = None;
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if trimmed.is_empty() && !in_fence && offset > 0 && chars_before <= max_chars {
            best = Some(offset);
        }
        if is_fence {
            in_fence = !in_fence;
        }
        chars_before += line.chars().count();
        offset += line.len();
    }
    let cut = best
        .or_else(|| text.char_indices().nth(max_chars).map(|(i, _)| i))
        .unwrap_or(text.len());
    (text[..cut].trim_end(), text[cut..].trim_start())
}

const CSS: &str = r#"
:root, body[data-theme="dark"] {
  color-scheme: dark;
  --bg: #0f1218; --fg: #dfe6ef; --muted: #94a0b0; --dim: #5d6675;
  --panel: #12161e; --panel-2: #161b24; --border: #232a36; --border-strong: #2a3140;
  --accent: #5b9dff; --user-accent: #e0b25c; --turn-accent: #6fce9f;
  --link-dot: rgba(91, 157, 255, 0.55); --user-bar: rgba(224, 178, 92, 0.65);
  --accent-bar: rgba(91, 157, 255, 0.65); --turn-glow: rgba(111, 206, 159, 0.6);
  --accent-soft: rgba(91, 157, 255, 0.12); --user-soft: rgba(224, 178, 92, 0.12);
  --code-bg: #0b0e14; --code-inline: #1a202b; --table-head: #1a202b;
  --zebra: rgba(255, 255, 255, 0.025);
  --header-bg: rgba(15, 18, 24, 0.92); --target-glow: rgba(91, 157, 255, 0.28);
  --chip-shadow: 0 8px 22px rgba(0, 0, 0, 0.55); --header-h: 84px;
}
body[data-theme="light"] {
  color-scheme: light;
  --bg: #ffffff; --fg: #1f2328; --muted: #57606a; --dim: #8c959f;
  --panel: #f7f9fb; --panel-2: #eef1f4; --border: #d8dee4; --border-strong: #d0d7de;
  --accent: #0969da; --user-accent: #bf8700; --turn-accent: #1a7f37;
  --link-dot: rgba(9, 105, 218, 0.55); --user-bar: rgba(191, 135, 0, 0.65);
  --accent-bar: rgba(9, 105, 218, 0.65); --turn-glow: rgba(26, 127, 55, 0.5);
  --accent-soft: rgba(9, 105, 218, 0.08); --user-soft: rgba(191, 135, 0, 0.10);
  --code-bg: #f6f8fa; --code-inline: #eff2f5; --table-head: #f6f8fa;
  --zebra: rgba(0, 0, 0, 0.015);
  --header-bg: rgba(255, 255, 255, 0.94); --target-glow: rgba(9, 105, 218, 0.18);
  --chip-shadow: 0 8px 22px rgba(31, 35, 40, 0.16); --header-h: 84px;
}
* { box-sizing: border-box; }
body { margin: 0 auto; max-width: 860px; padding: 22px 24px 90px;
  font: 16px/1.8 "Segoe UI", "Source Han Sans SC", "Microsoft YaHei", system-ui, sans-serif;
  letter-spacing: 0.015em;
  background: var(--bg); color: var(--fg); text-rendering: optimizeLegibility; }
a { color: var(--accent); text-decoration: none;
  border-bottom: 1px dotted var(--link-dot); }
a:hover { border-bottom-style: solid; }
code, pre { font-family: Consolas, "Cascadia Mono", "JetBrains Mono", "Courier New", monospace; }

header.meta { position: sticky; top: 0; z-index: 30;
  background: var(--header-bg);
  -webkit-backdrop-filter: blur(10px); backdrop-filter: blur(10px);
  border-bottom: 1px solid var(--border);
  padding: 6px 0 10px; margin-bottom: 26px; }
header.meta h1 { font-size: 19px; line-height: 1.35; margin: 0 0 6px; }
header.meta .sub { font-size: 11.5px; color: var(--muted); line-height: 1.55; }
header.meta .sub code { font-size: 11px; }

/* Left turn directory: a slim rail of dashes. Hover expands it to a FIXED
   width (--toc-w, sized for the widest "turn N") and swaps dashes for labels;
   scrolling highlights the current turn. */
.toc { position: fixed; left: 10px; top: 50%; transform: translateY(-50%);
  width: 26px; height: 190px; overflow-y: auto; overflow-x: hidden;
  display: flex; flex-direction: column; gap: 2px; padding: 6px 4px;
  z-index: 20; border-radius: 10px;
  transition: width .16s ease, height .16s ease;
  scrollbar-width: thin; scrollbar-color: var(--border-strong) transparent;
  -webkit-mask-image: linear-gradient(to bottom, transparent 0, #000 20px,
    #000 calc(100% - 20px), transparent 100%);
  mask-image: linear-gradient(to bottom, transparent 0, #000 20px,
    #000 calc(100% - 20px), transparent 100%); }
.toc::-webkit-scrollbar { width: 4px; }
.toc::-webkit-scrollbar-track { background: transparent; }
.toc::-webkit-scrollbar-thumb { background: var(--border-strong); border-radius: 2px; }
.toc a { display: flex; align-items: center; height: 18px; flex: 0 0 auto;
  color: var(--dim); font-size: 12px; text-decoration: none; border: 0;
  border-radius: 6px; white-space: nowrap; padding: 0 4px; }
.toc a .dash { display: inline-block; width: 14px; text-align: center;
  transition: transform .18s ease, color .18s ease; }
.toc a:nth-child(odd) .dash { opacity: .9; }
.toc a:nth-child(even) .dash { opacity: .5; }
.toc a .lbl { display: none; padding-left: 2px; color: var(--muted); }
.toc a:hover { background: var(--code-inline); color: var(--fg); }
.toc:not(:hover) a.active .dash { color: var(--turn-accent); font-weight: 700;
  opacity: 1; transform: scale(1.5);
  text-shadow: 0 0 10px var(--turn-glow); }
.toc:hover { width: var(--toc-w); height: min(72vh, 640px);
  background: var(--header-bg);
  -webkit-backdrop-filter: blur(8px); backdrop-filter: blur(8px);
  box-shadow: var(--chip-shadow); }
.toc:hover a .lbl { display: inline; }
.toc:hover a .dash { display: none; }
.toc:hover a.active .lbl { color: var(--turn-accent); font-weight: 700; }
@media (max-width: 1080px) { .toc { display: none; } }

.turn { border: 1px solid var(--border); background: var(--panel); border-radius: 12px;
  margin: 26px 0; padding: 8px 20px 20px;
  scroll-margin-top: calc(var(--header-h, 84px) + 12px); }
.turn:target { border-color: var(--accent); box-shadow: 0 0 0 3px var(--target-glow); }
.turn-head { font-size: 12px; color: var(--muted); padding: 12px 0 10px;
  display: flex; gap: 10px; align-items: center;
  border-bottom: 1px dashed var(--border); margin-bottom: 8px; }
.badge { background: none; border: 0; padding: 0;
  color: var(--turn-accent); font-weight: 700; font-size: 13px; letter-spacing: .02em; }
.rid { color: var(--dim); font-size: 11.5px; overflow: hidden;
  text-overflow: ellipsis; white-space: nowrap; }

.msg { margin: 22px 0; }
.msg .role { display: flex; align-items: center; gap: 8px; margin: 0 0 10px; }
.msg .role .dot { width: 9px; height: 9px; border-radius: 50%;
  background: currentColor; box-shadow: 0 0 10px currentColor; }
.msg .role .name { font-size: 14px; font-weight: 700; letter-spacing: .1em;
  padding: 2px 12px; border-radius: 999px; border: 1px solid currentColor; }
.msg-user .role { color: var(--user-accent); }
.msg-user .role .name { background: var(--user-soft); }
.msg-assistant .role { color: var(--accent); }
.msg-assistant .role .name { background: var(--accent-soft); }
.msg .body { padding-left: 16px; border-left: 3px solid var(--border-strong); }
.msg-user .body { border-left-color: var(--user-bar); }
.msg-assistant .body { border-left-color: var(--accent-bar); }

/* Reading rhythm: uniform spacing between top-level blocks (Markdown-app feel). */
.md { font-size: 16px; }
.md > * { margin: 0; }
.md > * + * { margin-top: 1.15em; }
.md > :is(h1, h2, h3, h4, h5, h6) { line-height: 1.45; font-weight: 700; }
.md > * + :is(h1, h2, h3, h4, h5, h6) { margin-top: 1.7em; }
.md > h1 { font-size: 1.5em; }
.md > h2 { font-size: 1.32em; }
.md > h3 { font-size: 1.16em; }
.md > h4, .md > h5, .md > h6 { font-size: 1.03em; }
.md > ul, .md > ol { padding-left: 1.6em; }
.md li { margin: .4em 0; }
.md li > p { margin: 0; }
.md li > ul, .md li > ol { margin: .35em 0 .1em; }
.md > blockquote { padding: .6em 1em; border-left: 3px solid var(--border-strong);
  background: var(--panel-2); border-radius: 0 8px 8px 0; color: var(--muted); }
.md > blockquote > * { margin: 0; }
.md > blockquote > * + * { margin-top: .8em; }
.md > hr { border: 0; border-top: 1px solid var(--border); }
.md code { background: var(--code-inline); padding: 1.5px 6px; border-radius: 6px; font-size: .87em; }
.md > pre { background: var(--code-bg); border: 1px solid var(--border); border-radius: 10px;
  padding: 14px 16px; overflow-x: auto; line-height: 1.65; }
.md > pre code { background: none; padding: 0; font-size: 13px; }
.md > table { border-collapse: collapse; display: block; overflow-x: auto;
  font-size: .93em; line-height: 1.6; }
.md th, .md td { border: 1px solid var(--border-strong); padding: 7px 14px; }
.md th { background: var(--table-head); font-weight: 600; text-align: left; }
.md tbody tr:nth-child(even) { background: var(--zebra); }

.md.preview { opacity: .98; }
details.more { margin-top: 1.15em; }
summary { display: inline-flex; align-items: center; gap: 8px; cursor: pointer;
  -webkit-user-select: none; user-select: none; padding: 6px 14px;
  border-radius: 999px; font-size: 12.5px;
  background: var(--code-inline); border: 1px solid var(--border-strong); color: var(--muted); }
summary:hover { color: var(--fg); border-color: var(--accent); }
summary::-webkit-details-marker { display: none; }
summary .chev::before { content: "▸"; }
details[open] summary .chev::before { content: "▾"; }
summary .lbl::after { content: ""; }
/* Per-language label contents are injected after this stylesheet. */
details[open] summary .lbl::after { content: ""; }
summary .meta { color: var(--dim); }
/* Sticky "收起" while open: follows the scroll, clamped by this message box
   and parked right below the sticky header. */
details.more[open] > summary { position: sticky;
  top: calc(var(--header-h, 84px) + 10px); z-index: 5;
  box-shadow: var(--chip-shadow); }
details.more[open] > summary + .md { margin-top: 1.15em; }

.empty { color: var(--dim); font-style: italic; margin: .4em 0; }
footer { margin-top: 40px; font-size: 12px; color: var(--dim); text-align: center; }
img { max-width: 100%; border-radius: 8px; }
::selection { background: var(--target-glow); }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_contains_anchor_and_escapes() {
        let row = SessionRow {
            id: "s1".into(),
            source_path: "C:/x/s1.jsonl".into(),
            workspace_hash: None,
            is_empty_window: false,
            project_key: "k".into(),
            project_label: "proj".into(),
            open_target: None,
            title: Some("标题 <b>".into()),
            created_ms: Some(1000),
            file_size: 1,
            file_mtime_ms: 1,
            turns: 1,
        };
        let turns = vec![TurnRow {
            idx: 3,
            request_id: Some("r1".into()),
            user_text: "问 <script>x</script>".into(),
            assistant_text: "答 **粗**".into(),
        }];
        let html = render_page(&row, &turns, Some(3), true, Lang::Zh);
        assert!(html.contains("data-theme=\"light\""));
        assert!(html.contains("lang=\"zh-CN\""));
        assert!(html.contains("class=\"name\">用户"));
        assert!(html.contains("class=\"name\">助手"));
        assert!(html.contains("id=\"turn-3\""));
        assert!(html.contains("class=\"toc\""), "left turn directory exists");
        assert_eq!(html.matches("href=\"#turn-3\"").count(), 1);
        assert!(html.contains("&lt;script&gt;"), "content HTML is escaped");
        assert_eq!(
            html.matches("<script").count(),
            1,
            "only the template's own TOC script may appear"
        );
        assert!(html.contains("--toc-w:"), "toc reserves a fixed hover width");
        assert!(html.contains("<strong>粗</strong>"));
        assert!(html.contains("标题 &lt;b&gt;"));
    }

    #[test]
    fn long_messages_collapse_unless_targeted() {
        let row = SessionRow {
            id: "s2".into(),
            source_path: "C:/x/s2.jsonl".into(),
            workspace_hash: None,
            is_empty_window: false,
            project_key: "k".into(),
            project_label: "proj".into(),
            open_target: None,
            title: Some("长消息".into()),
            created_ms: Some(1000),
            file_size: 1,
            file_mtime_ms: 1,
            turns: 1,
        };
        let long = "长".repeat(2000);
        let turns = vec![TurnRow {
            idx: 0,
            request_id: None,
            user_text: "短问题".into(),
            assistant_text: long,
        }];
        let html = render_page(&row, &turns, None, false, Lang::Zh);
        assert!(html.contains("<details class=\"more\">"), "long message should be collapsed");
        assert!(!html.contains("<details class=\"more\" open>"));
        assert!(html.contains("2000 字符"));
        assert!(html.contains("class=\"md preview\""), "preview renders above the disclosure");
        assert_eq!(html.matches("<details").count(), 1, "only the long one collapses");

        let html2 = render_page(&row, &turns, Some(0), false, Lang::Zh);
        assert!(html2.contains("<details class=\"more\" open>"), "targeted turn starts expanded");
    }

    #[test]
    fn english_page_uses_english_labels() {
        let row = SessionRow {
            id: "s3".into(),
            source_path: "C:/x/s3.jsonl".into(),
            workspace_hash: None,
            is_empty_window: false,
            project_key: "k".into(),
            project_label: "proj".into(),
            open_target: None,
            title: Some("hello".into()),
            created_ms: Some(1000),
            file_size: 1,
            file_mtime_ms: 1,
            turns: 1,
        };
        let turns = vec![TurnRow {
            idx: 0,
            request_id: None,
            user_text: "question".into(),
            assistant_text: "answer".into(),
        }];
        let html = render_page(&row, &turns, None, false, Lang::En);
        assert!(html.contains("lang=\"en\""));
        assert!(html.contains("class=\"name\">User"));
        assert!(html.contains("class=\"name\">Assistant"));
        assert!(html.contains("Show the rest"));
        assert!(html.contains("Collapse"));
        assert!(html.contains("generated by recall"));
        assert!(html.contains("turn index"));
    }

    #[test]
    fn preview_split_prefers_paragraph_boundary() {
        let text = format!("{}\n\n{}", "甲".repeat(300), "乙".repeat(300));
        let (p, r) = split_for_preview(&text, 400);
        assert!(p.chars().count() <= 400);
        assert!(p.ends_with('甲'));
        assert!(r.starts_with('乙'));
    }

    #[test]
    fn preview_split_skips_fenced_code_blanks() {
        let text = format!(
            "intro\n\n```\n{}\n\n{}\n```\n\nafter {}",
            "x".repeat(200),
            "y".repeat(200),
            "z".repeat(500)
        );
        let (p, r) = split_for_preview(&text, 500);
        assert!(p.trim_end().ends_with("```"), "cut must not land inside the fence: {p}");
        assert!(r.starts_with("after"));
    }
}
