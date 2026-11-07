// Fixed JXA resource, run with osascript -l JavaScript. Native ID focus only.
// argv: exact application PID, window/tab/terminal IDs, settle budget (ms),
// frontmost capability, max windows, max tabs, max terminals.
// No input, keypress, title/cwd matching or application-only fallback.
// FOCUSED <target window/tab/terminal> <front/selected/focused IDs>
//         <target tab selected> <app frontmost or -> <frontmost app PID>.
ObjC.import('Foundation');
ObjC.import('AppKit');

function uniqueIds(ids) {
    return new Set(ids).size === ids.length;
}

function checkId(id) {
    if (typeof id !== 'string' || !id.length || id.length > 256 || /[\x00-\x1f\x7f]/.test(id)) {
        throw Error('Invalid Ghostty native ID');
    }
    return id;
}

function nativeId(object) {
    return checkId(object.id());
}

function nested(level, parents) {
    return Array.isArray(level) && level.length === parents.length && level.every(Array.isArray);
}

function total(lists) {
    return lists.reduce((sum, list) => sum + list.length, 0);
}

// One Apple event per level: every window's, tab's and terminal's ID, each
// read answered from one state of the application, read from the windows
// down and each level bounded before its children are asked for. Null when
// separately read levels do not line up, which means the application changed.
function structure(app, maxWindows, maxTabs, maxTerminals) {
    const windows = app.windows.id();
    if (windows.length > maxWindows) throw Error('Ghostty inventory exceeds limit');
    const tabs = app.windows.tabs.id();
    if (!nested(tabs, windows)) return null;
    if (total(tabs) > maxTabs) throw Error('Ghostty inventory exceeds limit');
    const terminals = app.windows.tabs.terminals.id();
    if (!nested(terminals, windows) || !terminals.every((lists, w) => nested(lists, tabs[w]))) return null;
    if (total(terminals.flat()) > maxTerminals) throw Error('Ghostty inventory exceeds limit');
    const appTerminals = app.terminals.id();
    if (appTerminals.length > maxTerminals) throw Error('Ghostty inventory exceeds limit');
    return { windows, tabs, terminals, appTerminals };
}

// The same IDs read from the terminals up, so a change to any level after its
// first read is seen by the second read of that level or of a parent.
function reread(app) {
    const appTerminals = app.terminals.id();
    const terminals = app.windows.tabs.terminals.id();
    const tabs = app.windows.tabs.id();
    const windows = app.windows.id();
    return { windows, tabs, terminals, appTerminals };
}

function run(argv) {
    if (argv.length !== 9) throw Error('Ghostty focus arguments');
    const appPid = Number(argv[0]);
    if (!Number.isInteger(appPid) || appPid <= 0 || appPid > 2147483647) throw Error('Invalid application PID');
    const targetWindowId = argv[1];
    const targetTabId = argv[2];
    const targetTerminalId = argv[3];
    const budgetMs = Number(argv[4]);
    const canReadFrontmost = argv[5] === 'true';
    const maxWindows = Number(argv[6]);
    const maxTabs = Number(argv[7]);
    const maxTerminals = Number(argv[8]);
    const app = Application(appPid);
    if (!app.running()) return 'GONE';
    const s = structure(app, maxWindows, maxTabs, maxTerminals);
    if (s === null) return 'CHANGED';
    const windowIds = s.windows;
    const appTerminalIds = s.appTerminals;
    windowIds.forEach(checkId);
    appTerminalIds.forEach(checkId);
    if (!uniqueIds(windowIds) || !uniqueIds(appTerminalIds)) return 'CHANGED';
    const seenTabs = [];
    const seenTerminals = [];
    const matches = [];
    windowIds.forEach((windowId, w) => {
        s.tabs[w].forEach((tabId, t) => {
            seenTabs.push(checkId(tabId));
            s.terminals[w][t].forEach(terminalId => {
                seenTerminals.push(checkId(terminalId));
                if (terminalId === targetTerminalId) matches.push({ windowId, tabId });
            });
        });
    });
    if (!matches.length) return 'GONE';
    if (matches.length > 1) return ['AMBIGUOUS', matches.length].join('\t');
    if (!uniqueIds(seenTabs) || !uniqueIds(seenTerminals) ||
        seenTerminals.length !== appTerminalIds.length ||
        !appTerminalIds.every(id => seenTerminals.includes(id))) return 'CHANGED';
    if (JSON.stringify(reread(app)) !== JSON.stringify(s)) return 'CHANGED';
    if (matches[0].windowId !== targetWindowId || matches[0].tabId !== targetTabId) return 'CHANGED';

    const targetWindow = app.windows.byId(targetWindowId);
    const targetTab = targetWindow.tabs.byId(targetTabId);
    const targetTerminal = targetTab.terminals.byId(targetTerminalId);
    if (nativeId(targetWindow) !== targetWindowId || nativeId(targetTab) !== targetTabId ||
        nativeId(targetTerminal) !== targetTerminalId) return 'CHANGED';
    // Native focus selects the pane but does not itself activate Ghostty.
    // Activate this exact PID, then select and verify the exact native triple.
    app.activate();
    app.focus(targetTerminal);

    // Native Space/window transitions may settle asynchronously. The bounded
    // subprocess enforces the full budget, including every native query.
    let readback;
    for (let remaining = Math.floor(budgetMs / 25); ; remaining--) {
        const frontWindow = app.frontWindow();
        const selectedTab = frontWindow.selectedTab();
        readback = {
            front: nativeId(frontWindow), selected: nativeId(selectedTab),
            focused: nativeId(selectedTab.focusedTerminal()), targetSelected: targetTab.selected(),
            frontmost: canReadFrontmost ? app.frontmost() : '-',
            frontmostPid: Number($.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier)
        };
        if ((readback.front === targetWindowId && readback.selected === targetTabId &&
             readback.focused === targetTerminalId && readback.targetSelected === true &&
             (!canReadFrontmost || readback.frontmost === true) &&
             readback.frontmostPid === appPid) || remaining <= 0) break;
        $.NSThread.sleepForTimeInterval(0.025);
    }
    return ['FOCUSED', nativeId(targetWindow), nativeId(targetTab), nativeId(targetTerminal),
        readback.front, readback.selected, readback.focused, readback.targetSelected,
        readback.frontmost, readback.frontmostPid].join('\t');
}
