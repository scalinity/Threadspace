import json, os, shutil, subprocess, sys, time
src, root, label, n = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
os.makedirs(root, exist_ok=True)
rows = []
for i in range(n):
    path = os.path.join(root, f"{label}-{i}")
    shutil.copy2(src, path)
    times = []
    for attempt in range(3):
        t0 = time.perf_counter_ns()
        r = subprocess.run([path], stdin=subprocess.DEVNULL, capture_output=True)
        times.append(round((time.perf_counter_ns() - t0) / 1e6, 3))
    rows.append({"copy": i, "ino": os.stat(path).st_ino, "ms": times, "exit": r.returncode})
    time.sleep(0.3)
print(json.dumps({"label": label, "src": src, "rows": rows}))
