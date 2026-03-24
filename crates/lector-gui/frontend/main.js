// Tauri IPC — use the global __TAURI__ injected by Tauri
const { invoke } = window.__TAURI__.core;

// State
let treeCursor = 0;
let flatTree = [];
let focusedPane = 'tree'; // 'tree' or 'viewer'
let showTree = true;
let showToc = false;
let tocMode = 'auto'; // 'side', 'replace', 'auto'
let tocReplace = false; // resolved mode: true = replace tree, false = side panel
let pendingPrefix = null;
let currentFile = null;
let fontSize = 16;
let tocEntries = [];
let tocCursor = 0;

// Init
async function init() {
  const config = await invoke('get_config');
  fontSize = config.font.size;
  applyFontSize();
  if (config.ui.theme) {
    document.body.className = 'theme-' + config.ui.theme;
  }
  if (config.ui.toc_replace) {
    tocMode = 'replace';
  }
  resolveTocMode();

  const version = await invoke('get_version');
  document.getElementById('help-version').textContent = version;

  const initialPath = await invoke('get_initial_path');
  await loadTree();

  if (initialPath) {
    await openFile(initialPath);
  }
}

// Tree
async function loadTree() {
  const response = await invoke('get_tree');
  flatTree = response.entries;
  renderTree();
}

function renderTree() {
  const pane = document.getElementById('tree-pane');
  pane.innerHTML = '';

  flatTree.forEach((entry, idx) => {
    const btn = document.createElement('button');
    btn.className = 'tree-entry';
    if (idx === treeCursor && focusedPane === 'tree') btn.classList.add('selected');
    if (entry.is_current) btn.classList.add('current');
    if (entry.is_dir) btn.classList.add('directory');

    const indent = '\u00A0\u00A0'.repeat(entry.depth);
    const icon = entry.is_dir ? (entry.is_expanded ? '▾ ' : '▸ ') : '\u00A0\u00A0';
    btn.textContent = indent + icon + entry.name;

    btn.addEventListener('click', () => {
      treeCursor = idx;
      if (entry.is_dir) {
        toggleDir(entry.path);
      } else {
        openFile(entry.path);
      }
    });

    pane.appendChild(btn);
  });

  scrollTreeCursorIntoView();
}

function scrollTreeCursorIntoView() {
  const pane = document.getElementById('tree-pane');
  const entries = pane.querySelectorAll('.tree-entry');
  if (entries[treeCursor]) {
    entries[treeCursor].scrollIntoView({ block: 'nearest' });
  }
}

async function toggleDir(path) {
  await invoke('toggle_dir', { path });
  await loadTree();
}

async function saveCurrentPosition() {
  if (currentFile) {
    const offset = document.getElementById('viewer-content').scrollTop;
    await invoke('save_position', { path: currentFile, offset });
  }
}

async function openFile(path) {
  // Save position of previous file
  await saveCurrentPosition();

  const response = await invoke('open_file', { path });
  currentFile = path;
  document.getElementById('viewer-header').textContent = response.filename;
  document.getElementById('viewer-content').innerHTML = response.html;

  // Restore saved scroll position
  const saved = await invoke('load_position', { path });
  if (saved != null) {
    document.getElementById('viewer-content').scrollTop = saved;
  } else {
    document.getElementById('viewer-content').scrollTop = 0;
  }

  focusedPane = 'viewer';
  await loadTree();
  if (showToc) refreshToc();
  applyAnnotations();
}

function closeFile() {
  saveCurrentPosition();
  currentFile = null;
  document.getElementById('viewer-header').textContent = '';
  document.getElementById('viewer-content').innerHTML =
    '<p class="placeholder">Open a file from the tree to start reading.</p>';
  loadTree();
}

async function reloadFile() {
  const result = await invoke('reload_file');
  if (result) {
    const scrollPos = document.getElementById('viewer-content').scrollTop;
    document.getElementById('viewer-header').textContent = result.filename;
    document.getElementById('viewer-content').innerHTML = result.html;
    document.getElementById('viewer-content').scrollTop = scrollPos;
    showToast('Reloaded');
  }
}

async function setTreeRootFromCursor() {
  const entry = flatTree[treeCursor];
  if (!entry) return;
  const response = await invoke('set_tree_root', { path: entry.path });
  flatTree = response.entries;
  treeCursor = 0;
  renderTree();
  showToast('Root: ' + (flatTree[0]?.name || ''));
}

async function refreshTree() {
  const response = await invoke('refresh_tree');
  flatTree = response.entries;
  renderTree();
  showToast('Tree refreshed');
}

// Open path bar (C-x C-f) with tab completion
function showOpenBar() {
  const bar = document.getElementById('open-bar');
  const input = document.getElementById('open-input');
  bar.classList.remove('hidden');
  input.value = '';
  input.focus();
}

function hideOpenBar() {
  document.getElementById('open-bar').classList.add('hidden');
}

async function handleOpenPath(path) {
  if (!path) return;
  try {
    const result = await invoke('open_path', { path });
    if (result) {
      // File was opened
      currentFile = result.path || path;
      document.getElementById('viewer-header').textContent = result.filename;
      document.getElementById('viewer-content').innerHTML = result.html;
      document.getElementById('viewer-content').scrollTop = 0;
      focusedPane = 'viewer';
    } else {
      // Directory was changed
      currentFile = null;
      document.getElementById('viewer-header').textContent = '';
      document.getElementById('viewer-content').innerHTML =
        '<p class="placeholder">Open a file from the tree to start reading.</p>';
    }
    await loadTree();
    treeCursor = 0;
  } catch (err) {
    showToast('Error: ' + err);
  }
}

document.getElementById('open-input').addEventListener('keydown', async (e) => {
  if (e.key === 'Enter') {
    const path = document.getElementById('open-input').value;
    hideOpenBar();
    await handleOpenPath(path);
    e.preventDefault();
  } else if (e.key === 'Escape' || (e.ctrlKey && e.key === 'g')) {
    hideOpenBar();
    e.preventDefault();
  } else if (e.key === 'Tab') {
    // Tab completion
    e.preventDefault();
    const input = document.getElementById('open-input');
    const completions = await invoke('complete_path', { input: input.value });
    if (completions.length === 1) {
      input.value = completions[0];
    } else if (completions.length > 1) {
      // Find common prefix
      let common = completions[0];
      for (const c of completions) {
        while (!c.startsWith(common)) {
          common = common.slice(0, -1);
        }
      }
      if (common.length > input.value.length) {
        input.value = common;
      } else {
        showToast(completions.map(c => c.split('/').pop() || c).join('  '));
      }
    }
  }
  e.stopPropagation();
});

// Visual file browser (C-o)
let browserEntries = [];
let browserCursor = 0;
let browserDir = '';

async function showFileBrowser() {
  // Start from current file's directory or tree root
  const startDir = currentFile
    ? currentFile.substring(0, currentFile.lastIndexOf('/'))
    : flatTree[0]?.path || '/';
  await loadBrowserDir(startDir);
  document.getElementById('file-browser').classList.remove('hidden');
  document.getElementById('browser-filter-input').value = '';
  document.getElementById('browser-filter-input').focus();
}

function hideFileBrowser() {
  document.getElementById('file-browser').classList.add('hidden');
}

async function loadBrowserDir(dir) {
  browserDir = dir;
  browserEntries = await invoke('browse_directory', { path: dir });
  browserCursor = 0;
  document.getElementById('file-browser-header').textContent = dir;
  renderBrowser('');
}

function renderBrowser(filter) {
  const list = document.getElementById('file-browser-list');
  list.innerHTML = '';

  const filtered = filter
    ? browserEntries.filter(e => e.name.toLowerCase().includes(filter.toLowerCase()))
    : browserEntries;

  if (browserCursor >= filtered.length) browserCursor = Math.max(0, filtered.length - 1);

  filtered.forEach((entry, idx) => {
    const btn = document.createElement('button');
    btn.className = 'browser-entry';
    if (idx === browserCursor) btn.classList.add('selected');
    if (entry.is_dir) btn.classList.add('directory');

    const icon = entry.is_dir ? '📁 ' : '  ';
    btn.textContent = icon + entry.name;

    btn.addEventListener('click', () => {
      browserCursor = idx;
      selectBrowserEntry(filtered);
    });

    list.appendChild(btn);
  });

  // Scroll selected into view
  const selected = list.querySelector('.selected');
  if (selected) selected.scrollIntoView({ block: 'nearest' });
}

async function selectBrowserEntry(filtered) {
  const entry = filtered[browserCursor];
  if (!entry) {
    // No entry selected (empty list or no matches) — open the current directory
    hideFileBrowser();
    await handleOpenPath(browserDir);
    return;
  }

  if (entry.is_dir) {
    await loadBrowserDir(entry.path);
    document.getElementById('browser-filter-input').value = '';
  } else {
    hideFileBrowser();
    await handleOpenPath(entry.path);
  }
}

document.getElementById('browser-filter-input').addEventListener('keydown', async (e) => {
  const filter = document.getElementById('browser-filter-input').value;
  const filtered = filter
    ? browserEntries.filter(en => en.name.toLowerCase().includes(filter.toLowerCase()))
    : browserEntries;

  if (e.key === 'Enter') {
    await selectBrowserEntry(filtered);
    e.preventDefault();
  } else if (e.key === 'Escape' || (e.ctrlKey && e.key === 'g')) {
    hideFileBrowser();
    e.preventDefault();
  } else if ((e.ctrlKey && e.key === 'n') || e.key === 'ArrowDown') {
    browserCursor = Math.min(browserCursor + 1, filtered.length - 1);
    renderBrowser(filter);
    e.preventDefault();
  } else if ((e.ctrlKey && e.key === 'p') || e.key === 'ArrowUp') {
    browserCursor = Math.max(browserCursor - 1, 0);
    renderBrowser(filter);
    e.preventDefault();
  }
  e.stopPropagation();
});

document.getElementById('browser-filter-input').addEventListener('input', (e) => {
  browserCursor = 0;
  renderBrowser(e.target.value);
});

// Search
function showSearch() {
  const bar = document.getElementById('search-bar');
  const input = document.getElementById('search-input');
  bar.classList.remove('hidden');
  input.value = '';
  document.getElementById('search-count').textContent = '';
  input.focus();
}

function hideSearch() {
  document.getElementById('search-bar').classList.add('hidden');
  // Clear selection/highlights
  window.getSelection().removeAllRanges();
}

function performSearch(query, forward) {
  if (!query) return;
  // window.find(string, caseSensitive, backwards, wrapAround)
  const found = window.find(query, false, !forward, true);
  document.getElementById('search-count').textContent = found ? '' : 'not found';
}

document.getElementById('search-input').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') {
    performSearch(document.getElementById('search-input').value, !e.shiftKey);
    e.preventDefault();
  } else if (e.key === 'Escape' || (e.ctrlKey && e.key === 'g')) {
    hideSearch();
    e.preventDefault();
  } else if (e.ctrlKey && e.key === 's') {
    // C-s while in search = find next
    performSearch(document.getElementById('search-input').value, true);
    e.preventDefault();
  } else if (e.ctrlKey && e.key === 'r') {
    // C-r while in search = find previous
    performSearch(document.getElementById('search-input').value, false);
    e.preventDefault();
  }
  e.stopPropagation();
});

document.getElementById('search-input').addEventListener('input', (e) => {
  // Incremental search: search as you type
  performSearch(e.target.value, true);
});

// Help
function toggleHelp() {
  document.getElementById('help-overlay').classList.toggle('hidden');
}

// Font size — goes through Rust to persist in config
function applyFontSize() {
  document.documentElement.style.setProperty('--font-size', fontSize + 'px');
  showToast(`Font size: ${fontSize}px`);
}

function showToast(msg) {
  let el = document.getElementById('toast');
  if (!el) {
    el = document.createElement('div');
    el.id = 'toast';
    el.style.cssText = 'position:fixed;bottom:32px;right:32px;background:var(--bg-header);color:var(--fg);padding:6px 16px;border-radius:4px;border:1px solid var(--border);font-size:14px;opacity:0;transition:opacity 0.2s;z-index:200;pointer-events:none';
    document.body.appendChild(el);
  }
  el.textContent = msg;
  el.style.opacity = '1';
  clearTimeout(el._timer);
  el._timer = setTimeout(() => { el.style.opacity = '0'; }, 1500);
}

async function adjustFontSize(delta) {
  fontSize = await invoke('adjust_font_size', { delta });
  applyFontSize();
}

async function resetFontSize() {
  fontSize = await invoke('adjust_font_size', { delta: 0.0 });
  applyFontSize();
}

// Toggle tree pane
function toggleTreePane() {
  showTree = !showTree;
  document.getElementById('tree-pane').classList.toggle('hidden', !showTree);
}

// Table of Contents
function resolveTocMode() {
  if (tocMode === 'replace') {
    tocReplace = true;
  } else if (tocMode === 'side') {
    tocReplace = false;
  } else {
    // auto: replace if viewport < 100 characters wide (~800px)
    tocReplace = window.innerWidth < 800;
  }
}

async function toggleToc() {
  showToc = !showToc;
  resolveTocMode();

  const app = document.getElementById('app');
  const toc = document.getElementById('toc-pane');
  const tree = document.getElementById('tree-pane');
  const viewer = document.getElementById('viewer-pane');

  if (showToc) {
    await refreshToc();
    if (tocReplace) {
      // Replace tree: put ToC where tree is (before viewer)
      tree.classList.add('hidden');
      app.insertBefore(toc, viewer);
      toc.classList.remove('hidden');
      toc.style.borderLeft = 'none';
      toc.style.borderRight = '1px solid var(--border)';
    } else {
      // Side mode: ToC on opposite side of tree (after viewer)
      app.appendChild(toc);
      toc.classList.remove('hidden');
      toc.style.borderLeft = '1px solid var(--border)';
      toc.style.borderRight = 'none';
    }
  } else {
    toc.classList.add('hidden');
    if (tocReplace) {
      tree.classList.toggle('hidden', !showTree);
    }
  }
}

function cycleTocMode() {
  if (tocMode === 'auto') tocMode = 'side';
  else if (tocMode === 'side') tocMode = 'replace';
  else tocMode = 'auto';

  resolveTocMode();
  showToast('ToC mode: ' + tocMode + (tocMode === 'auto' ? (tocReplace ? ' (replace)' : ' (side)') : ''));

  // Re-apply if ToC is visible
  if (showToc) {
    showToc = false;
    toggleToc();
  }
}

async function refreshToc() {
  const headings = await invoke('get_headings');
  // Add annotations as ToC entries
  let annotations = [];
  if (currentFile) {
    annotations = await invoke('get_annotations', { filePath: currentFile });
  }
  tocEntries = [
    ...headings.map(h => ({ type: 'heading', ...h })),
    ...annotations.map(a => ({
      type: 'annotation',
      text: a.comment || a.selected_text,
      id: 'annotation-' + a.id,
      level: 0,
      annotation: a,
    })),
  ];
  tocCursor = 0;
  renderToc();
}

function renderToc() {
  const pane = document.getElementById('toc-pane');
  pane.innerHTML = '';

  if (tocEntries.length === 0) {
    pane.innerHTML = '<div style="padding:8px;color:var(--fg-dim);font-size:0.85em">No headings</div>';
    return;
  }

  let hasAnnotations = tocEntries.some(e => e.type === 'annotation');
  let annotationHeaderShown = false;

  tocEntries.forEach((h, idx) => {
    // Show a separator before annotations
    if (h.type === 'annotation' && !annotationHeaderShown && hasAnnotations) {
      annotationHeaderShown = true;
      const sep = document.createElement('div');
      sep.className = 'toc-separator';
      sep.textContent = '— Notes —';
      pane.appendChild(sep);
    }

    const btn = document.createElement('button');
    if (h.type === 'annotation') {
      btn.className = 'toc-entry toc-annotation';
      btn.textContent = '📝 ' + h.text;
    } else {
      btn.className = 'toc-entry h' + h.level;
      btn.textContent = h.text;
    }
    if (idx === tocCursor && focusedPane === 'toc') btn.classList.add('selected');
    btn.addEventListener('click', () => {
      tocCursor = idx;
      if (h.type === 'annotation' && h.annotation) {
        scrollToAnnotation(h.annotation);
      } else {
        scrollToHeading(h.id);
      }
      renderToc();
    });
    pane.appendChild(btn);
  });

  // Scroll selected into view
  const selected = pane.querySelector('.selected');
  if (selected) selected.scrollIntoView({ block: 'nearest' });
}

function scrollToHeading(id) {
  if (!id) return;
  const el = document.getElementById(id);
  if (el) {
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    el.style.transition = 'background 0.3s';
    el.style.background = 'var(--bg-selected)';
    setTimeout(() => { el.style.background = ''; }, 1500);
  }
}

function scrollToAnnotation(ann) {
  // Scroll to the annotation's position in the viewer using text offset
  const viewer = document.getElementById('viewer-content');
  const viewerText = viewer.textContent;
  const offset = lineColToOffset(viewerText, ann.start_line, ann.start_col);

  // Walk text nodes to find the right position
  const treeWalker = document.createTreeWalker(viewer, NodeFilter.SHOW_TEXT);
  let totalOffset = 0;
  while (treeWalker.nextNode()) {
    const node = treeWalker.currentNode;
    if (totalOffset + node.length > offset) {
      // Found the node — scroll its parent into view
      const parent = node.parentElement;
      if (parent) {
        parent.scrollIntoView({ behavior: 'smooth', block: 'center' });
        parent.style.transition = 'background 0.3s';
        parent.style.background = 'var(--bg-selected)';
        setTimeout(() => { parent.style.background = ''; }, 1500);
      }
      return;
    }
    totalOffset += node.length;
  }
}

// Cycle theme
async function cycleTheme() {
  const newTheme = await invoke('cycle_theme');
  document.body.className = 'theme-' + newTheme;
}

// Scrolling
function scrollViewer(lines) {
  const el = document.getElementById('viewer-content');
  el.scrollBy(0, lines * 24);
}

function pageViewer(pages) {
  const el = document.getElementById('viewer-content');
  el.scrollBy(0, pages * el.clientHeight * 0.9);
}

// Keybindings
document.addEventListener('keydown', (e) => {
  // Don't intercept when dir input is focused
  // Don't intercept when an input field is focused
  const active = document.activeElement;
  if (active && (active.tagName === 'INPUT' || active.tagName === 'TEXTAREA')) return;

  const helpVisible = !document.getElementById('help-overlay').classList.contains('hidden');

  // C-g: emacs "cancel" — dismiss overlays, cancel chords (never starts meta prefix)
  if (e.ctrlKey && e.key === 'g') {
    if (helpVisible) { toggleHelp(); e.preventDefault(); return; }
    pendingPrefix = null;
    e.preventDefault();
    return;
  }

  // Escape: dismiss overlays, cancel chords, or start ESC-as-Meta prefix
  if (e.key === 'Escape') {
    if (helpVisible) { toggleHelp(); e.preventDefault(); return; }
    if (pendingPrefix) { pendingPrefix = null; e.preventDefault(); return; }
    // ESC acts as Meta prefix (emacs: ESC x = M-x)
    pendingPrefix = 'escape';
    e.preventDefault();
    return;
  }

  // C-h toggles help even when help is visible
  if (e.ctrlKey && e.key === 'h') { toggleHelp(); e.preventDefault(); return; }

  // Block other keys when help is visible
  if (helpVisible) return;

  // Chord handling: C-x prefix
  if (e.ctrlKey && !e.altKey && e.key === 'x') {
    pendingPrefix = 'x';
    e.preventDefault();
    return;
  }

  if (pendingPrefix === 'escape') {
    // ESC prefix: treat next key as Alt+key
    pendingPrefix = null;
    // Synthesize an Alt key event by dispatching to the Alt handler
    const key = e.key;
    e.preventDefault();
    switch (key) {
      case 'v': pageViewer(-1); return;
      case 't': cycleTheme(); return;
      case '<': case ',':
        document.getElementById('viewer-content').scrollTop = 0; return;
      case '>': case '.':
        { const el = document.getElementById('viewer-content'); el.scrollTop = el.scrollHeight; } return;
    }
    return;
  }

  if (pendingPrefix === 'x') {
    pendingPrefix = null;
    if (e.ctrlKey && e.key === 'f') { showOpenBar(); e.preventDefault(); return; }
    if (e.ctrlKey && e.key === 'a') { showAnnotationsList(); e.preventDefault(); return; }
    if (e.ctrlKey && e.key === 'd') { setTreeRootFromCursor(); e.preventDefault(); return; }
    if (e.ctrlKey && e.key === 't') { toggleToc(); e.preventDefault(); return; }
    if (e.ctrlKey && e.key === 'm') { cycleTocMode(); e.preventDefault(); return; }
    if (e.ctrlKey && e.key === 'c') { saveCurrentPosition().then(() => invoke('quit')); e.preventDefault(); return; }
    e.preventDefault();
    return;
  }

  // Single keybindings
  if (e.ctrlKey && !e.altKey) {
    switch (e.key) {
      case 'n':
        if (focusedPane === 'viewer') scrollViewer(1);
        else if (focusedPane === 'tree') { treeCursor = Math.min(treeCursor + 1, flatTree.length - 1); renderTree(); }
        else if (focusedPane === 'toc') { tocCursor = Math.min(tocCursor + 1, tocEntries.length - 1); renderToc(); }
        e.preventDefault(); return;
      case 'p':
        if (focusedPane === 'viewer') scrollViewer(-1);
        else if (focusedPane === 'tree') { treeCursor = Math.max(treeCursor - 1, 0); renderTree(); }
        else if (focusedPane === 'toc') { tocCursor = Math.max(tocCursor - 1, 0); renderToc(); }
        e.preventDefault(); return;
      case 'v':
        pageViewer(1);
        e.preventDefault(); return;
      case 'f':
        if (focusedPane === 'tree') {
          const entry = flatTree[treeCursor];
          if (entry && entry.is_dir && !entry.is_expanded) toggleDir(entry.path);
        } else if (focusedPane === 'toc' && tocEntries[tocCursor]) {
          const tocEntry = tocEntries[tocCursor];
          if (tocEntry.type === 'annotation' && tocEntry.annotation) scrollToAnnotation(tocEntry.annotation);
          else scrollToHeading(tocEntry.id);
        } else {
          scrollViewer(1);
        }
        e.preventDefault(); return;
      case 'b':
        if (focusedPane === 'tree') {
          const entry = flatTree[treeCursor];
          if (entry && entry.is_dir && entry.is_expanded) toggleDir(entry.path);
        } else {
          scrollViewer(-1);
        }
        e.preventDefault(); return;
      case 'w':
        closeFile();
        e.preventDefault(); return;
      case 'r':
        if (focusedPane === 'tree') refreshTree();
        else reloadFile();
        e.preventDefault(); return;
      case 's':
        showSearch();
        e.preventDefault(); return;
      case 'o':
        showFileBrowser();
        e.preventDefault(); return;
      case 'm':
        showAnnotationBar();
        e.preventDefault(); return;
      case 't':
        toggleTreePane();
        e.preventDefault(); return;
      case '=':
      case '+':
        adjustFontSize(2);
        e.preventDefault(); return;
      case '-':
        adjustFontSize(-2);
        e.preventDefault(); return;
      case '0':
        resetFontSize();
        e.preventDefault(); return;
    }
  }

  if (e.altKey && !e.ctrlKey) {
    switch (e.key) {
      case 'v':
        pageViewer(-1);
        e.preventDefault(); return;
      case 't':
        cycleTheme();
        e.preventDefault(); return;
      case '<':
      case ',':
        document.getElementById('viewer-content').scrollTop = 0;
        e.preventDefault(); return;
      case '>':
      case '.':
        const el = document.getElementById('viewer-content');
        el.scrollTop = el.scrollHeight;
        e.preventDefault(); return;
    }
  }

  // Non-modifier keys
  if (!e.ctrlKey && !e.altKey && !e.metaKey) {
    switch (e.key) {
      case 'Tab': {
        const visible = [];
        if (showTree) visible.push('tree');
        visible.push('viewer');
        if (showToc) visible.push('toc');
        const idx = visible.indexOf(focusedPane);
        focusedPane = visible[(idx + 1) % visible.length];
        renderTree();
        if (showToc) renderToc();
        e.preventDefault(); return;
      }
      case 'ArrowUp':
        if (focusedPane === 'viewer') scrollViewer(-1);
        else if (focusedPane === 'tree') { treeCursor = Math.max(treeCursor - 1, 0); renderTree(); }
        else if (focusedPane === 'toc') { tocCursor = Math.max(tocCursor - 1, 0); renderToc(); }
        e.preventDefault(); return;
      case 'ArrowDown':
        if (focusedPane === 'viewer') scrollViewer(1);
        else if (focusedPane === 'tree') { treeCursor = Math.min(treeCursor + 1, flatTree.length - 1); renderTree(); }
        else if (focusedPane === 'toc') { tocCursor = Math.min(tocCursor + 1, tocEntries.length - 1); renderToc(); }
        e.preventDefault(); return;
      case 'ArrowLeft':
        if (focusedPane === 'tree') {
          const entry = flatTree[treeCursor];
          if (entry && entry.is_dir && entry.is_expanded) toggleDir(entry.path);
        }
        e.preventDefault(); return;
      case 'ArrowRight':
      case 'Enter':
        if (focusedPane === 'tree' && flatTree[treeCursor]) {
          const entry = flatTree[treeCursor];
          if (entry.is_dir) toggleDir(entry.path);
          else openFile(entry.path);
        } else if (focusedPane === 'toc' && tocEntries[tocCursor]) {
          const tocEntry = tocEntries[tocCursor];
          if (tocEntry.type === 'annotation' && tocEntry.annotation) scrollToAnnotation(tocEntry.annotation);
          else scrollToHeading(tocEntry.id);
        }
        e.preventDefault(); return;
      case 'q':
        saveCurrentPosition().then(() => invoke('quit'));
        e.preventDefault(); return;
    }
  }
});

// -- Annotations --

let pendingAnnotationRange = null;

let editingAnnotationId = null; // set when editing an existing annotation
let activeAnnotationRanges = []; // [{range, annotation}] for click detection

function showAnnotationModal(selectedText, existingComment, annotationId) {
  editingAnnotationId = annotationId || null;

  document.getElementById('annotation-selected-text').textContent = selectedText;
  document.getElementById('annotation-input').value = existingComment || '';
  document.getElementById('annotation-modal').classList.remove('hidden');
  document.getElementById('annotation-delete-btn').classList.toggle('hidden', !annotationId);
  document.getElementById('annotation-input').focus();
}

function hideAnnotationModal() {
  document.getElementById('annotation-modal').classList.add('hidden');
  pendingAnnotationRange = null;
  editingAnnotationId = null;
}

function showAnnotationBar() {
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || !sel.rangeCount) {
    showToast('Select text first');
    return;
  }

  const range = sel.getRangeAt(0);
  const selectedText = sel.toString().trim();
  if (!selectedText) {
    showToast('Select text first');
    return;
  }

  const viewer = document.getElementById('viewer-content');
  const preRange = document.createRange();
  preRange.setStart(viewer, 0);
  preRange.setEnd(range.startContainer, range.startOffset);
  const startOffset = preRange.toString().length;
  const endOffset = startOffset + selectedText.length;

  if (!currentFile) return;

  pendingAnnotationRange = { selectedText, startOffset, endOffset };
  showAnnotationModal(selectedText, '', null);
}

async function saveAnnotation(comment) {
  if (!pendingAnnotationRange || !currentFile) return;

  const { selectedText, startOffset, endOffset } = pendingAnnotationRange;

  // Map offsets to source line+col using the viewer's text content
  const viewerText = document.getElementById('viewer-content').textContent;
  const startPos = offsetToLineCol(viewerText, startOffset);
  const endPos = offsetToLineCol(viewerText, endOffset);

  try {
    await invoke('save_annotation', {
      filePath: currentFile,
      startLine: startPos.line,
      startCol: startPos.col,
      endLine: endPos.line,
      endCol: endPos.col,
      selectedText,
      comment: comment || '',
      color: 'yellow',
    });
    showToast('Annotation saved');
    await applyAnnotations();
  } catch (err) {
    showToast('Error: ' + err);
  }

  pendingAnnotationRange = null;
}

function offsetToLineCol(text, offset) {
  let line = 0;
  let col = 0;
  for (let i = 0; i < offset && i < text.length; i++) {
    if (text[i] === '\n') {
      line++;
      col = 0;
    } else {
      col++;
    }
  }
  return { line, col };
}

function lineColToOffset(text, line, col) {
  let currentLine = 0;
  let offset = 0;
  for (let i = 0; i < text.length; i++) {
    if (currentLine === line) {
      return offset + col;
    }
    if (text[i] === '\n') {
      currentLine++;
      offset = i + 1;
    }
  }
  return offset + col;
}

async function applyAnnotations() {
  activeAnnotationRanges = [];
  if (!currentFile || !CSS.highlights) return;

  // Clear existing highlights
  CSS.highlights.clear();

  const annotations = await invoke('get_annotations', { filePath: currentFile });
  if (!annotations.length) return;

  const viewer = document.getElementById('viewer-content');
  const viewerText = viewer.textContent;
  const treeWalker = document.createTreeWalker(viewer, NodeFilter.SHOW_TEXT);

  // Build a list of text nodes with their offsets
  const textNodes = [];
  let totalOffset = 0;
  while (treeWalker.nextNode()) {
    const node = treeWalker.currentNode;
    textNodes.push({ node, start: totalOffset, end: totalOffset + node.length });
    totalOffset += node.length;
  }

  // Group annotations by color
  const byColor = {};
  for (const ann of annotations) {
    const startOff = lineColToOffset(viewerText, ann.start_line, ann.start_col);
    const endOff = lineColToOffset(viewerText, ann.end_line, ann.end_col);

    // Find text nodes that overlap this range
    const range = document.createRange();
    let rangeSet = false;

    for (const tn of textNodes) {
      if (tn.end <= startOff) continue;
      if (tn.start >= endOff) break;

      if (!rangeSet) {
        const localStart = Math.max(0, startOff - tn.start);
        range.setStart(tn.node, localStart);
        rangeSet = true;
      }
      const localEnd = Math.min(tn.node.length, endOff - tn.start);
      range.setEnd(tn.node, localEnd);
    }

    if (rangeSet) {
      const color = ann.color || 'yellow';
      if (!byColor[color]) byColor[color] = [];
      byColor[color].push(range);
      activeAnnotationRanges.push({ range, annotation: ann });
    }
  }

  // Register highlights
  for (const [color, ranges] of Object.entries(byColor)) {
    if (ranges.length > 0) {
      const highlight = new Highlight(...ranges);
      CSS.highlights.set('annotation-' + color, highlight);
    }
  }
}

async function showAnnotationsList() {
  // Ensure ToC is visible
  if (!showToc) {
    await toggleToc();
  }
  // Refresh to ensure annotations are loaded
  await refreshToc();
  // Find the first annotation entry and move cursor there
  const annotIdx = tocEntries.findIndex(e => e.type === 'annotation');
  if (annotIdx >= 0) {
    tocCursor = annotIdx;
    focusedPane = 'toc';
    renderToc();
    renderTree();
  } else {
    showToast('No annotations for this file');
  }
}

document.getElementById('annotation-input').addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) {
    const comment = document.getElementById('annotation-input').value;
    const pending = pendingAnnotationRange;
    const editId = editingAnnotationId;
    hideAnnotationModal();
    pendingAnnotationRange = pending;
    if (editId) {
      // Update: delete old and save new
      invoke('delete_annotation', { id: editId }).then(() => saveAnnotation(comment));
    } else {
      saveAnnotation(comment);
    }
    e.preventDefault();
  } else if (e.key === 'Escape' || (e.ctrlKey && e.key === 'g')) {
    hideAnnotationModal();
    e.preventDefault();
  }
  e.stopPropagation();
});

document.getElementById('annotation-save-btn').addEventListener('click', () => {
  const comment = document.getElementById('annotation-input').value;
  const pending = pendingAnnotationRange;
  const editId = editingAnnotationId;
  hideAnnotationModal();
  pendingAnnotationRange = pending;
  if (editId) {
    invoke('delete_annotation', { id: editId }).then(() => saveAnnotation(comment));
  } else {
    saveAnnotation(comment);
  }
});

document.getElementById('annotation-delete-btn').addEventListener('click', async () => {
  if (editingAnnotationId) {
    await invoke('delete_annotation', { id: editingAnnotationId });
    hideAnnotationModal();
    await applyAnnotations();
    if (showToc) await refreshToc();
    showToast('Annotation deleted');
  }
});

document.getElementById('annotation-cancel-btn').addEventListener('click', () => {
  hideAnnotationModal();
});

// Intercept clicks in the viewer — check for annotation highlights first, then links
document.getElementById('viewer-content').addEventListener('click', async (e) => {
  // Check if click is on an annotated range
  const sel = window.getSelection();
  if (sel && sel.rangeCount && activeAnnotationRanges.length) {
    const clickRange = sel.getRangeAt(0);
    for (const { range, annotation } of activeAnnotationRanges) {
      if (range.isPointInRange(clickRange.startContainer, clickRange.startOffset)) {
        // Clicked on an annotation — open modal to view/edit
        const viewer = document.getElementById('viewer-content');
        const viewerText = viewer.textContent;
        const startOff = lineColToOffset(viewerText, annotation.start_line, annotation.start_col);
        const endOff = lineColToOffset(viewerText, annotation.end_line, annotation.end_col);
        pendingAnnotationRange = {
          selectedText: annotation.selected_text,
          startOffset: startOff,
          endOffset: endOff,
        };
        showAnnotationModal(annotation.selected_text, annotation.comment, annotation.id);
        e.preventDefault();
        return;
      }
    }
  }

  // Check for links
  const link = e.target.closest('a[href]');
  if (link) {
    e.preventDefault();
    const href = link.getAttribute('href');
    if (!href) return;
    const localPath = await invoke('resolve_link', { url: href });
    if (localPath) {
      await handleOpenPath(localPath);
    }
  }
});

// Listen for file watcher events — auto-refresh tree when files change
if (window.__TAURI__.event) {
  window.__TAURI__.event.listen('tree-changed', async () => {
    await loadTree();
  });
}

// Start
init();
