#!/usr/bin/env python3
"""A deterministic stand-in for `codex app-server`, speaking the subset of
the App Server protocol Rodtty uses (JSON-RPC 2.0, one message per line).

Prompts steer it: `APPROVE` asks for a command approval first, `SLOW` runs
until interrupted, `FAIL` fails the turn; anything else is echoed back.
"""
import json
import sys
import threading
import time

lock = threading.Lock()
turns = {}
pending = {}
counter = [0]


def send(message):
    with lock:
        sys.stdout.write(json.dumps(message) + "\n")
        sys.stdout.flush()


def notify(method, params):
    send({"jsonrpc": "2.0", "method": method, "params": params})


def finish(thread, turn, status, text=None, error=None):
    if text is not None:
        notify("item/started", {"threadId": thread, "turnId": turn, "item": {"type": "agentMessage", "id": "msg-" + turn}})
        for i in range(0, len(text), 4):
            notify("item/agentMessage/delta", {"threadId": thread, "turnId": turn, "itemId": "msg-" + turn, "delta": text[i:i + 4]})
        notify("item/completed", {"threadId": thread, "turnId": turn, "item": {"type": "agentMessage", "id": "msg-" + turn, "text": text}})
    if error:
        notify("error", {"threadId": thread, "turnId": turn, "error": {"message": error}, "willRetry": False})
    turn_value = {"id": turn, "items": [], "status": status}
    if error:
        turn_value["error"] = {"message": error}
    notify("turn/completed", {"threadId": thread, "turn": turn_value})
    turns.pop(turn, None)


def run_turn(thread, turn, prompt):
    notify("turn/started", {"threadId": thread, "turn": {"id": turn, "items": [], "status": "inProgress"}})
    if "APPROVE" in prompt:
        request_id = "approval-" + turn
        pending[request_id] = (thread, turn)
        send({"jsonrpc": "2.0", "id": request_id, "method": "item/commandExecution/requestApproval",
              "params": {"threadId": thread, "turnId": turn, "itemId": "cmd-" + turn, "startedAtMs": 0,
                         "command": ["touch", "approved.txt"], "cwd": "."}})
        return
    if "SLOW" in prompt:
        turns[turn] = thread
        return
    if "FAIL" in prompt:
        finish(thread, turn, "failed", error="fixture failure")
        return
    finish(thread, turn, "completed", text="Echo: " + prompt)


for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    params = message.get("params") or {}
    mid = message.get("id")
    if method is None and mid in pending:
        thread, turn = pending.pop(mid)
        decision = (message.get("result") or {}).get("decision")
        notify("serverRequest/resolved", {"threadId": thread, "turnId": turn, "requestId": mid})
        finish(thread, turn, "completed", text="Approved: " + str(decision))
        continue
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {"userAgent": "fake-codex-app-server/1.0"}})
    elif method == "initialized":
        pass
    elif method in ("thread/start", "thread/resume"):
        thread = params.get("threadId") or "thr-fixture"
        send({"jsonrpc": "2.0", "id": mid, "result": {"thread": {"id": thread}, "cwd": params.get("cwd"), "model": "fixture",
              "modelProvider": "fixture", "approvalPolicy": "on-request", "approvalsReviewer": "user", "sandbox": {"type": "readOnly"}}})
        notify("thread/started", {"thread": {"id": thread}})
    elif method == "turn/start":
        counter[0] += 1
        turn = "turn-%d" % counter[0]
        prompt = "".join(i.get("text", "") for i in params.get("input", []))
        send({"jsonrpc": "2.0", "id": mid, "result": {"turn": {"id": turn, "items": [], "status": "inProgress"}}})
        threading.Thread(target=run_turn, args=(params.get("threadId"), turn, prompt)).start()
    elif method == "turn/interrupt":
        turn = params.get("turnId")
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
        if turn in turns:
            time.sleep(0.05)
            finish(params.get("threadId"), turn, "interrupted")
    elif mid is not None:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "unsupported: " + str(method)}})
