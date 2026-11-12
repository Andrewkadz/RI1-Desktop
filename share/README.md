# RI1 Share

Lets a friend chat with RI1 Desktop's model from a browser link, while the
model runs on your Mac.

## Start sharing

    brew install cloudflared          # once
    cd ~/Downloads/ri1-desktop/share
    ./share.sh

It prints a **link** and a **passcode**. Send them in separate messages.
Press **Ctrl-C** to stop: the link dies immediately, and you get a new link
and passcode next time.

## What your friend can do

Chat only, with your `ri1-llama:desktop` model and your RI1 Basin (with
sources). They cannot reach the Workshop, your files, or Ollama's controls.

## Things to know

- Your Mac must stay **on, awake, and running Ollama**. The script keeps it
  from sleeping, but closing the lid usually still sleeps it.
- Answers quote your Basin passages verbatim, so anyone with the link and
  passcode can read those passages.
- One answer at a time; a second person gets "busy, try again".
- 10 wrong passcodes in 10 minutes lock sharing for 15 minutes.
- The free Cloudflare link is temporary and random by design.

Files: `server.py` (the server), `basin_search.py` (same search as the app),
`index.html` (the chat page), `share.sh` (starts everything).
