// Recall Jump — open a historical local chat session by id.
//
// Findings driving v0.2 (upstream source + live test, 2026-10-18):
// - local session URI: vscode-chat-session://local/<b64url-nopad(id)>;
//   the scheme is registered with the editor resolver, so vscode.open reaches
//   ChatEditorInput.
// - BUT a chat session store is window-scoped (workspace window: only its own
//   workspaceStorage/<hash>/chatSessions; only an empty window reads
//   globalStorage/emptyWindowChatSessions). When the requested session cannot
//   be loaded, ChatEditorInput silently starts a NEW empty session.
// - Therefore: check scope first (via `recall show <id> --json --meta`); when
//   the session belongs to another window, write a pending-jump request and
//   open/focus that window — the extension there consumes it and jumps.
const vscode = require('vscode');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');

const l10n = vscode.l10n;

const UUID_RE = /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;
const PENDING_TTL_MS = 10 * 60 * 1000;
const RELEASES_URL = 'https://github.com/sfw222/vs-chat-recall/releases';
/// Set when the user answers "Don't show again" in the setup warning.
const SETUP_DISMISSED_KEY = 'recallJump.setupDismissed';

function cfg() {
  const c = vscode.workspace.getConfiguration('recall');
  return {
    exe: c.get('executable', '') || 'recall',
    db: c.get('db', ''),
    pending:
      c.get('pendingFile', '') || path.join(os.tmpdir(), 'recall-jump-request.json'),
  };
}

function toB64Url(sessionId) {
  return Buffer.from(sessionId, 'utf8')
    .toString('base64')
    .replace(/=+$/g, '')
    .replace(/\+/g, '-')
    .replace(/\//g, '_');
}

function uriForId(sessionId) {
  return vscode.Uri.from({
    scheme: 'vscode-chat-session',
    authority: 'local',
    path: '/' + toB64Url(sessionId),
  });
}

function runRecallJson(args) {
  return new Promise((resolve) => {
    const conf = cfg();
    if (!conf.exe) return resolve(undefined);
    const fullArgs = conf.db ? args.concat(['--db', conf.db]) : args;
    const opts = { windowsHide: true };
    if (!path.isAbsolute(conf.exe)) opts.shell = true;
    let child;
    try {
      child = spawn(conf.exe, fullArgs, opts);
    } catch {
      return resolve(undefined);
    }
    let out = '';
    child.stdout.on('data', (d) => (out += d));
    child.on('error', () => resolve(undefined));
    child.on('close', (code) => {
      if (code !== 0) return resolve(undefined);
      try {
        resolve(JSON.parse(out));
      } catch {
        resolve(undefined);
      }
    });
  });
}

async function describe(id) {
  const j = await runRecallJson(['show', id, '--json', '--meta']);
  return j && j.session ? j.session : undefined;
}

// ---------------------------------------------------------------------------
// Setup / first-run diagnostics
//
// The extension is useless without the `recall` CLI and an index, so probe
// both, and offer one-click fixes instead of failing silently (opening a
// session without the CLI would even start a blank chat tab, because the
// scope check cannot run).
// ---------------------------------------------------------------------------

function probeCli(exe) {
  return new Promise((resolve) => {
    if (!exe) return resolve({ ok: false });
    const opts = { windowsHide: true };
    if (!path.isAbsolute(exe)) opts.shell = true;
    let child;
    try {
      child = spawn(exe, ['--version'], opts);
    } catch {
      return resolve({ ok: false });
    }
    let out = '';
    child.stdout.on('data', (d) => (out += d));
    child.stderr.on('data', (d) => (out += d));
    child.on('error', () => resolve({ ok: false }));
    child.on('close', () => resolve({ ok: true, version: out.trim().split('\n')[0] }));
  });
}

/// 'unset' (no path configured) | 'missing' (path configured but not a file) | 'ok'
function dbState(dbPath) {
  if (!dbPath) return 'unset';
  try {
    return fs.statSync(dbPath).isFile() ? 'ok' : 'missing';
  } catch {
    return 'missing';
  }
}

async function setupState() {
  const conf = cfg();
  return { conf, cli: await probeCli(conf.exe), db: dbState(conf.db) };
}

function isReady(st) {
  return st.cli.ok && st.db === 'ok';
}

async function openReleases() {
  await vscode.env.openExternal(vscode.Uri.parse(RELEASES_URL));
}

/// Interactive warning with one-click fixes; returns true once ready.
async function showSetup(context, st) {
  if (isReady(st)) return true;

  const missing = [];
  if (!st.cli.ok) missing.push(l10n.t('the recall CLI was not found ({0})', st.conf.exe));
  if (st.db === 'unset') missing.push(l10n.t('no index file is configured (recall.db)'));
  else if (st.db === 'missing') missing.push(l10n.t('the index file does not exist: {0}', st.conf.db));

  const pickExe = l10n.t('Select recall executable\u2026');
  const pickIndex = l10n.t('Select index file\u2026');
  const collectLabel = l10n.t('Run recall collect');
  const download = l10n.t('Open download page');
  const never = l10n.t("Don't show again");

  const buttons = st.cli.ok ? [pickIndex, collectLabel, never] : [pickExe, download, never];
  const before = `${st.cli.ok}|${st.db}`;
  const chosen = await vscode.window.showWarningMessage(
    l10n.t('recall-jump needs the recall CLI and its index. Missing: {0}', missing.join('; ')),
    ...buttons
  );

  if (chosen === pickExe) await pickExecutable();
  else if (chosen === download) await openReleases();
  else if (chosen === pickIndex) await pickDb();
  else if (chosen === collectLabel) await runCollect(context);
  else if (chosen === never) {
    await context.globalState.update(SETUP_DISMISSED_KEY, true);
    return false;
  }

  const after = await setupState();
  if (isReady(after)) return true;
  // Guide on to the *next* missing piece, but only when the action took effect
  // (a cancelled file dialog must not loop the warning).
  if (`${after.cli.ok}|${after.db}` !== before) return showSetup(context, after);
  return false;
}

/// Auto (activation) check: silent once the user dismissed the warning.
async function checkSetup(context, { auto }) {
  const st = await setupState();
  if (isReady(st)) return true;
  if (auto && context.globalState.get(SETUP_DISMISSED_KEY)) return false;
  return showSetup(context, st);
}

/// Command path: the user explicitly asked for a jump, so never no-op silently.
async function showSetupOrReady(context) {
  const st = await setupState();
  if (isReady(st)) return true;
  return showSetup(context, st);
}

async function setConfig(key, value) {
  await vscode.workspace
    .getConfiguration('recall')
    .update(key, value, vscode.ConfigurationTarget.Global);
}

async function pickExecutable() {
  const conf = cfg();
  const filters =
    process.platform === 'win32'
      ? { 'recall executable': ['exe'], 'All files': ['*'] }
      : { 'All files': ['*'] };
  const picked = await vscode.window.showOpenDialog({
    canSelectMany: false,
    openLabel: l10n.t('Use as the recall CLI'),
    filters,
  });
  if (!picked || picked.length === 0) return undefined;
  const exe = picked[0].fsPath;
  await setConfig('executable', exe);
  vscode.window.showInformationMessage(l10n.t('recall: executable set to {0}', exe));
  return exe;
}

async function pickDb() {
  const conf = cfg();
  const start = conf.db ? path.dirname(conf.db) : undefined;
  const picked = await vscode.window.showOpenDialog({
    canSelectMany: false,
    openLabel: l10n.t('Use as the recall index'),
    defaultUri: start ? vscode.Uri.file(start) : undefined,
    filters: { 'recall index': ['db'], 'All files': ['*'] },
  });
  if (!picked || picked.length === 0) return undefined;
  const db = picked[0].fsPath;
  await setConfig('db', db);
  vscode.window.showInformationMessage(l10n.t('recall: index file set to {0}', db));
  return db;
}

/// Build (or refresh) the index in a visible terminal; asks where to put the
/// index when none is configured yet.
async function runCollect(context) {
  const conf = cfg();
  let db = conf.db;
  if (dbState(db) !== 'ok') {
    const dir = db ? path.dirname(db) : undefined;
    const target = await vscode.window.showSaveDialog({
      saveLabel: l10n.t('Save the recall index here'),
      defaultUri: dir ? vscode.Uri.file(path.join(dir, 'recall.db')) : undefined,
      filters: { 'recall index': ['db'] },
    });
    if (!target) return;
    db = target.fsPath;
    await setConfig('db', db);
    vscode.window.showInformationMessage(l10n.t('recall: index file set to {0}', db));
  }

  try {
    fs.mkdirSync(path.dirname(db), { recursive: true });
  } catch {
    /* the CLI will report a proper error if it cannot write there */
  }

  const exe = conf.exe || 'recall';
  const quoted = /[\s"]/.test(exe) ? `"${exe}"` : exe;
  const term = vscode.window.createTerminal({ name: 'recall collect', cwd: path.dirname(db) });
  term.show(true);
  term.sendText(`${quoted} collect --db "${db}"`);
}

function inScope(meta) {
  const folders = (vscode.workspace.workspaceFolders || []).map((f) =>
    f.uri.fsPath.toLowerCase()
  );
  const wsFile = vscode.workspace.workspaceFile
    ? vscode.workspace.workspaceFile.fsPath.toLowerCase()
    : undefined;
  if (meta.is_empty_window) return folders.length === 0;
  if (wsFile && meta.open_target && wsFile === String(meta.open_target).toLowerCase()) {
    return true;
  }
  const key = String(meta.project_key || '');
  if (key.startsWith('hash:') || key === '<empty-window>') return false;
  return key.split('\n').some((root) => folders.includes(root));
}

async function openSessionUri(sessionId) {
  try {
    await vscode.commands.executeCommand('vscode.open', uriForId(sessionId));
    return true;
  } catch (e) {
    vscode.window.showErrorMessage(
      l10n.t(
        'recall: failed to open the session — {0} (fallback: `recall view <id> --open`)',
        e && e.message ? e.message : String(e)
      )
    );
    return false;
  }
}

async function openSession(arg, context) {
  // Without the CLI/index the scope check cannot run, and opening an unknown
  // session would silently start a blank chat tab — ask for setup instead.
  if (!(await showSetupOrReady(context))) return;

  let input = typeof arg === 'string' ? arg.trim() : '';
  if (!input) {
    input = (
      (await vscode.window.showInputBox({
        prompt: l10n.t('recall: enter a session id (UUID) or a full vscode-chat-session URI'),
        placeHolder: 'e.g. 3f2a9c1e-7b4d-4c8e-9a10-5d6f2b8c4e71',
      })) || ''
    ).trim();
  }
  if (!input) return;

  // A full URI: open as-is (best effort).
  if (input.startsWith('vscode-chat-session:')) {
    try {
      await vscode.commands.executeCommand('vscode.open', vscode.Uri.parse(input));
    } catch (e) {
      vscode.window.showErrorMessage(l10n.t('recall: open failed — {0}', e && e.message ? e.message : String(e)));
    }
    return;
  }

  const sessionId = UUID_RE.test(input) ? input : undefined;
  if (!sessionId) {
    vscode.window.showErrorMessage(l10n.t('recall: not a valid session UUID'));
    return;
  }

  const meta = await describe(sessionId);
  if (meta && !inScope(meta)) {
    const where = meta.is_empty_window
      ? l10n.t('an empty window')
      : l10n.t('workspace {0}', meta.open_target || l10n.t('(unknown)'));
    const action = await vscode.window.showWarningMessage(
      l10n.t(
        'Session "{0}" belongs to {1}. This window\u2019s session store does not contain it (opening it directly would only start a blank new session).',
        meta.title || sessionId.slice(0, 8),
        where
      ),
      l10n.t('Open in its window and jump')
    );
    if (action) await scheduleJump(meta);
    return;
  }

  await openSessionUri(sessionId);
}

async function scheduleJump(meta) {
  const conf = cfg();
  try {
    fs.mkdirSync(path.dirname(conf.pending), { recursive: true });
    fs.writeFileSync(
      conf.pending,
      JSON.stringify(
        {
          session_id: meta.id,
          title: meta.title,
          open_target: meta.open_target,
          is_empty_window: meta.is_empty_window,
          project_key: meta.project_key,
          ts: Date.now(),
        },
        null,
        2
      )
    );
  } catch (e) {
    vscode.window.showErrorMessage(l10n.t('recall: cannot write the jump request — {0}', e && e.message ? e.message : String(e)));
    return;
  }
  if (meta.is_empty_window || !meta.open_target) {
    await vscode.commands.executeCommand('workbench.action.newWindow');
  } else {
    await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(meta.open_target), {
      forceNewWindow: true,
    });
  }
}

function tryPendingJump() {
  const conf = cfg();
  let req;
  try {
    req = JSON.parse(fs.readFileSync(conf.pending, 'utf8'));
  } catch {
    return;
  }
  if (!req || !req.session_id) return;
  if (Date.now() - (req.ts || 0) > PENDING_TTL_MS) {
    try {
      fs.unlinkSync(conf.pending);
    } catch {
      /* ignore */
    }
    return;
  }
  if (!inScope(req)) return; // not this window's session — leave it for the right one
  try {
    fs.unlinkSync(conf.pending);
  } catch {
    /* ignore */
  }
  openSessionUri(req.session_id);
}

function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand('recall.openSession', (arg) => openSession(arg, context))
  );
  context.subscriptions.push(vscode.commands.registerCommand('recall.pickDb', () => pickDb()));
  context.subscriptions.push(
    vscode.commands.registerCommand('recall.pickExecutable', () => pickExecutable())
  );
  context.subscriptions.push(vscode.commands.registerCommand('recall.collect', () => runCollect(context)));
  context.subscriptions.push(
    vscode.commands.registerCommand('recall.checkSetup', async () => {
      if (await checkSetup(context, { auto: false })) {
        vscode.window.showInformationMessage(
          l10n.t('recall: setup looks good (index: {0})', cfg().db)
        );
      }
    })
  );

  // Consume pending jumps when this (possibly just opened/focused) window matches.
  context.subscriptions.push(
    vscode.window.onDidChangeWindowState((e) => {
      if (e.focused) setTimeout(tryPendingJump, 150);
    })
  );
  setTimeout(tryPendingJump, 2500);

  // First-run diagnostics: only while the CLI or the index is missing.
  setTimeout(() => checkSetup(context, { auto: true }), 3000);
}

function deactivate() {}

module.exports = { activate, deactivate };
