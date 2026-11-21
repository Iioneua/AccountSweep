# Instruction reference (`next.v1`)

`accountsweep next --agent` prints exactly one of these. All examples use fake data. The full contract is `accountsweep schemas show next.v1`.

## ask_user

```json
{"schema":"accountsweep.next.v1","phase":"setup","do":"ask_user","key":"consent","input":"yes_no",
 "say":"Before I start: in Basic mode I (your AI agent) will search your Gmail ...  Do you agree?",
 "answer_with":["accountsweep","answer","consent"]}
```

- Ask the user `say`. For `yes_no`, they answer yes or no. For `choice` / `multi_choice`, show each option's `label`. Map their reply to option `value`s (comma-separated for several).
- Then run `answer_with` plus the value: `accountsweep answer consent yes --agent`.
- If the user's reply is unclear, ask again. Never pick for them.

## connector_read

```json
{"schema":"accountsweep.next.v1","phase":"discover","do":"connector_read","connector":"gmail",
 "queries":[{"id":"signups","q":"subject:(\"welcome to\" OR ...)","max_results":60}, ...],
 "fields":["message_id","from","subject","date"],
 "ingest_with":["accountsweep","ingest","--kind","gmail_headers"],
 "say":"Run each search with your Gmail connector. ..."}
```

For **each** query, in order:

1. Search Gmail with `q`, asking for at most `max_results` messages.
2. Build one JSON object, using only the four fields per message:
   ```json
   {"schema":"accountsweep.ingest.gmail_headers.v1","query_id":"signups","items":[
     {"message_id":"<abc@shop.example>","from":"Example Shop <no-reply@shop.example>",
      "subject":"Welcome to Example Shop","date":"Tue, 02 Mar 2021 10:00:00 +0000"}
   ]}
   ```
   - `message_id`: the message's id from your connector (any stable id is fine).
   - `from`: the From header exactly as given.
   - `date`: the date as given (RFC 2822, ISO 8601, or Unix milliseconds are all accepted).
   - No results: `"items": []`. You still send it, so the tool knows the search ran.
3. Pipe it to `ingest_with` (quoted heredoc; see SKILL.md). The reply looks like:
   ```json
   {"schema":"accountsweep.envelope.v1","command":"ingest","ok":true,
    "data":{"query_id":"signups","received":5,"stored":4,"ignored_personal":1,"ignored_unparsed":0,"items_total":4,"queries_left":6},"warnings":[]}
   ```
4. After all queries, call `accountsweep next --agent` again.

If the connector isn't available or the user declines, run `accountsweep skip gmail --agent`.

## run

```json
{"schema":"accountsweep.next.v1","phase":"enrich","do":"run","command":["accountsweep","breaches","--agent"]}
```

Run the argv exactly as given, then call `next` again.

## human_step

```json
{"schema":"accountsweep.next.v1","phase":"act","do":"human_step","say":"Please sign in in the browser window, then tell me when you're done."}
```

Tell the user `say`, wait for them, then call `next` again.

## say

Relay the text, then call `next` again.

## done

```json
{"schema":"accountsweep.next.v1","phase":"report","do":"done","say":"Found 10 items linked to your email: 4 accounts, ..."}
```

Relay `say`. The sweep is complete. Calling `next` again returns the same `done`.

## browser_read / browser_act

Reserved for a later version (reading connected-apps pages and acting on approved items). If you receive one, tell the user this version of the skill can't do it yet, and stop.
