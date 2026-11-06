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

// One Apple event per level: every window's, tab's and terminal's ID, each
// read answered from one state of the application.
function structure(app) {
    return {
        windows: app.windows.id(),
        tabs: app.windows.tabs.id(),
        terminals: app.windows.tabs.terminals.id(),
        appTerminals: app.terminals.id(),
    };
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
    const s = structure(app);
    const windowIds = s.windows;
    const appTerminalIds = s.appTerminals;
    if (windowIds.length > maxWindows || appTerminalIds.length > maxTerminals) throw Error('Ghostty inventory exceeds limit');
    windowIds.forEach(checkId);
    appTerminalIds.forEach(checkId);
    if (!uniqueIds(windowIds) || !uniqueIds(appTerminalIds)) return 'CHANGED';
    // Window, tab and terminal lists read in separate events line up only if
    // nothing changed between them.
    if (!Array.isArray(s.tabs) || s.tabs.length !== windowIds.length ||
        !Array.isArray(s.terminals) || s.terminals.length !== windowIds.length ||
        !s.tabs.every((tabIds, w) => Array.isArray(tabIds) && Array.isArray(s.terminals[w]) &&
            s.terminals[w].length === tabIds.length && s.terminals[w].every(Array.isArray))) return 'CHANGED';
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
    if (seenTabs.length > maxTabs || seenTerminals.length > maxTerminals) throw Error('Ghostty inventory exceeds limit');
    if (!matches.length) return 'GONE';
    if (matches.length > 1) return ['AMBIGUOUS', matches.length].join('\t');
    if (!uniqueIds(seenTabs) || !uniqueIds(seenTerminals) ||
        seenTerminals.length !== appTerminalIds.length ||
        !appTerminalIds.every(id => seenTerminals.includes(id))) return 'CHANGED';
    // A change between any two reads invalidates the snapshot.
    if (JSON.stringify(structure(app)) !== JSON.stringify(s)) return 'CHANGED';
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
