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

async function openSession(arg) {
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
  context.subscriptions.push(vscode.commands.registerCommand('recall.openSession', openSession));
  // Consume pending jumps when this (possibly just opened/focused) window matches.
  context.subscriptions.push(
    vscode.window.onDidChangeWindowState((e) => {
      if (e.focused) setTimeout(tryPendingJump, 150);
    })
  );
  setTimeout(tryPendingJump, 2500);
}

function deactivate() {}

module.exports = { activate, deactivate };
