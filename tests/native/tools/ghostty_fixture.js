// Qualification only. The Python runner verifies this exact process incarnation
// before each call. Application(pid) avoids same-bundle application ambiguity.
function run(argv) {
  const app = Application(Number(argv[0]));
  const operation = argv[1];
  const configuration = { command: "/bin/sleep 300", waitAfterCommand: false };
  function inventory() {
    return {
      version: app.version(),
      windows: app.windows().map(w => ({
        id: w.id(), selectedTab: w.selectedTab().id(),
        tabs: w.tabs().map(t => ({
          id: t.id(), selected: t.selected(), focusedTerminal: t.focusedTerminal().id(),
          terminals: t.terminals().map(s => ({ id: s.id(), tty: s.tty(), pid: s.pid() }))
        }))
      }))
    };
  }
  function terminal(id) {
    const matches = app.terminals().filter(s => s.id() === id);
    if (matches.length !== 1) throw Error("terminal not found");
    return matches[0];
  }
  function pane(window, surface) {
    const tab = window.tabs().filter(t => t.terminals().some(s => s.id() === surface.id()));
    return {windowId: window.id(), tabId: tab.length === 1 ? tab[0].id() : null, terminalId: surface.id(), tty: surface.tty()};
  }
  function locate(id) {
    const found = [];
    app.windows().forEach(w => w.tabs().forEach(t => t.terminals().forEach(s => {
      if (s.id() === id) found.push({windowId: w.id(), tabId: t.id(), terminalId: s.id(), tty: s.tty()});
    })));
    if (found.length !== 1) throw Error("terminal not found");
    return found[0];
  }
  switch (operation) {
    case "inventory": return JSON.stringify(inventory());
    case "ids": return JSON.stringify({version: app.version(), ids: app.terminals().map(s => s.id()).sort()});
    case "create": {
      if (app.windows().length || app.terminals().length) throw Error("fixture requires empty application");
      const first = app.newWindow({withConfiguration: configuration});
      app.split(first.tabs()[0].terminals()[0], {direction: "right", withConfiguration: configuration});
      app.newTab({in: first, withConfiguration: configuration});
      app.newWindow({withConfiguration: configuration});
      return JSON.stringify(inventory());
    }
    case "focus": {
      const matches = app.terminals().filter(s => s.id() === argv[2]);
      if (matches.length !== 1) return JSON.stringify({focused: false, matches: matches.length});
      app.activate();
      app.focus(matches[0]);
      // Native transition readback is independent of the command result.
      let result;
      for (let i = 0; i < 40; i++) {
        const window = app.frontWindow(), tab = window.selectedTab();
        result = {focused: true, windowId: window.id(), tabId: tab.id(),
          terminalId: tab.focusedTerminal().id(), tabSelected: tab.selected(), frontmost: app.frontmost()};
        if (result.terminalId === argv[2] && result.tabSelected && result.frontmost) break;
        delay(0.025);
      }
      return JSON.stringify(result);
    }
    case "close": {
      // Every remaining terminal must be one the caller created; panes the
      // caller already closed are simply absent.
      const expected = JSON.parse(argv[2]);
      const actual = app.terminals().map(s => s.id()).sort();
      if (actual.some(id => !expected.includes(id))) throw Error("fixture terminal ownership changed");
      app.windows().forEach(w => app.closeWindow(w));
      // Window close completes asynchronously.
      for (let i = 0; i < 100 && (app.windows().length || app.terminals().length); i++) delay(0.05);
      if (app.windows().length || app.terminals().length) throw Error("fixture did not close");
      app.quit();
      return JSON.stringify({closed: actual});
    }
    // Pane operations for M2 session qualification. The caller launched this
    // process from a bundle path no other process uses (Apple events resolve
    // a PID through its bundle path) and proved it empty before creating.
    case "count": return JSON.stringify({version: app.version(), windows: app.windows().length, terminals: app.terminals().length});
    case "window": {
      const w = app.newWindow({withConfiguration: {initialWorkingDirectory: argv[2]}});
      return JSON.stringify(pane(w, w.tabs()[0].terminals()[0]));
    }
    case "tab": {
      const w = app.windows().filter(x => x.id() === argv[2]);
      if (w.length !== 1) throw Error("window not found");
      const t = app.newTab({in: w[0], withConfiguration: {initialWorkingDirectory: argv[3]}});
      return JSON.stringify(pane(w[0], t.terminals()[0]));
    }
    case "split": {
      const created = app.split(terminal(argv[2]), {direction: argv[3], withConfiguration: {initialWorkingDirectory: argv[4]}});
      return JSON.stringify(locate(created.id()));
    }
    case "locate": return JSON.stringify(locate(argv[2]));
    case "input": app.inputText(argv[3], {to: terminal(argv[2])}); return "{}";
    case "key": {
      const options = {to: terminal(argv[2])};
      if (argv[4]) options.modifiers = argv[4];
      app.sendKey(argv[3], options);
      return "{}";
    }
    case "action": return JSON.stringify({performed: app.performAction(argv[3], {on: terminal(argv[2])})});
    case "select-tab": {
      const tabs = [];
      app.windows().forEach(w => w.tabs().forEach(t => { if (t.id() === argv[2]) tabs.push(t); }));
      if (tabs.length !== 1) throw Error("tab not found");
      app.selectTab(tabs[0]);
      return "{}";
    }
    case "readback": {
      const window = app.frontWindow(), tab = window.selectedTab();
      return JSON.stringify({windowId: window.id(), tabId: tab.id(), terminalId: tab.focusedTerminal().id(),
        tabSelected: tab.selected(), frontmost: app.frontmost()});
    }
    case "close-terminal": {
      const owned = JSON.parse(argv[3]);
      if (!owned.includes(argv[2])) throw Error("terminal is not owned");
      app.close(terminal(argv[2]));
      return JSON.stringify({closed: argv[2]});
    }
    default: throw Error("unknown fixture operation");
  }
}
