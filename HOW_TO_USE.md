# How to Use mtgo-deckfinder — Simple Runbook

This tool finds recent, winning Magic: The Gathering Online (MTGO) decklists for
you, ranks them from best to worst, and saves the one you choose as a file you
can import straight into MTGO.

You do **not** need to know how to code. Just follow the steps and copy-paste the
commands. Every command is typed into a program called **Terminal** (on Mac) or
**Command Prompt / PowerShell** (on Windows). Type the command, then press
**Enter**.

---

## What you need

- A computer (Mac, Windows, or Linux).
- About 10 minutes for the one-time setup.
- An internet connection (the tool downloads decklists from the official MTGO
  website).

---

## Part 1 — One-time setup (do this once)

### Step 1: Install Rust

Rust is the free toolkit this program is built with. Installing it also gives you
the `cargo` command used in the next step.

- **Mac / Linux:** open Terminal, paste this line, press Enter, and accept the
  default option when asked:

  ```sh
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

- **Windows:** download and run the installer from
  <https://rustup.rs> and click through with the default options.

When it finishes, **close and reopen** your Terminal so the changes take effect.

### Step 2: Install the tool

Go into the project folder (the folder that contains this file) and install the
tool. In Terminal:

```sh
cd path/to/mtgo-deckfinder
cargo install --path .
```

> Tip: instead of typing the path, type `cd ` (with a space) and then drag the
> project folder onto the Terminal window — it fills in the path for you.

This takes a couple of minutes the first time. When it's done, you can run the
tool from anywhere by typing `mtgo-deckfinder`.

To check it worked, type:

```sh
mtgo-deckfinder --help
```

You should see a list of commands. Setup is complete.

---

## Part 2 — Using the tool (the 3 steps you'll repeat)

Every time you want decks, you do the same three things: **fetch**, **list**,
**export**.

### Step 1: Fetch — download recent decks for a format

Pick a format and download its recent winning decks. For example, Modern:

```sh
mtgo-deckfinder fetch modern
```

**What to expect:** the *first time ever* this is a little slow (it downloads a
card database, about 50 MB, and politely waits between requests so it doesn't
overload the MTGO website). This is normal. It only downloads the big database
once. You'll see something like:

```
Downloading MTGJSON card-name reference (~50 MB, cached afterwards)…
Fetching recent modern decks from mtgo.com…
Fetched and cached 439 modern decks (21 card-name warning(s)).
```

> The "card-name warning(s)" line is harmless. It just means a few cards are so
> new they aren't in the card database yet. The decks still work fine.

**Available formats** (use the exact word):

`standard`, `modern`, `pauper`, `pioneer`, `vintage`, `legacy`, `premodern`,
`duel-commander`, `contraption`, `limited`

### Step 2: List — see the decks ranked best-first

```sh
mtgo-deckfinder list modern
```

You'll see a table, best deck at the top:

```
  #  score  date        event        result   player              source
  1  1.000  2026-06-28  Challenge    #1       KingHairy           wotc-mtgo
  2  0.981  2026-06-27  Challenge    #1       gazmon48            wotc-mtgo
  3  0.973  2026-06-28  Challenge    #3       Polikasoll          wotc-mtgo
  ...
```

How to read it:

- **#** — the deck's rank. **#1 is the best pick.**
- **score** — higher is better (combines how recent, how strong the finish, and
  how trustworthy the source is).
- **date** — when the event happened.
- **event** — Challenge and League decks are the strongest.
- **result** — `#1` means it won the event; `5-0` means an undefeated league run.
- **player** — who piloted it.

Want to see more than the default 20 rows? Add `--limit`:

```sh
mtgo-deckfinder list modern --limit 50
```

### Step 3: Export — save a deck to import into MTGO

Pick a number from the list (the **#** column) and export it. To save the best
deck (#1):

```sh
mtgo-deckfinder export modern 1
```

This creates a file called **`deck.txt`** in your current folder. To choose a
different deck, use its number — for example the 3rd-best:

```sh
mtgo-deckfinder export modern 3
```

Want to name the file yourself? Add `--out`:

```sh
mtgo-deckfinder export modern 1 --out my-modern-deck.txt
```

---

## Part 3 — Import the deck into MTGO

1. Open the saved file (e.g. `deck.txt`) and **copy all the text**, **or** keep
   the file handy.
2. In the MTGO client, go to your **Collection / Decks**.
3. Use **Import Deck** and select the file (or paste the text).
4. The deck appears in MTGO, ready to play.

The tool only creates the importable file — it never controls MTGO or buys cards
for you.

---

## Getting the best results

- **Pick from the top.** The list is already sorted best-first. #1 is the
  strongest recent deck. The top 5–10 are all excellent choices.
- **Refresh for the very latest.** The tool reuses what it downloaded for a while
  so it's fast. To force it to grab brand-new decks (e.g. after a big tournament
  weekend), add `--refresh`:

  ```sh
  mtgo-deckfinder fetch modern --refresh
  ```

- **Browse before you commit.** Use `list ... --limit 50` to see more options,
  then export the number you like.
- **Try different formats.** Just swap the word: `fetch pauper`, then
  `list pauper`, then `export pauper 1`.

---

## Quick cheat sheet

| I want to… | Type this |
|------------|-----------|
| Download recent Modern decks | `mtgo-deckfinder fetch modern` |
| Get the newest decks (force refresh) | `mtgo-deckfinder fetch modern --refresh` |
| See the ranked list | `mtgo-deckfinder list modern` |
| See more rows | `mtgo-deckfinder list modern --limit 50` |
| Save the best deck | `mtgo-deckfinder export modern 1` |
| Save the 5th-best deck | `mtgo-deckfinder export modern 5` |
| Save with a custom filename | `mtgo-deckfinder export modern 1 --out burn.txt` |

(Swap `modern` for any format you like.)

---

## If something goes wrong

- **"no cached modern decks — run `fetch modern` first"**
  You tried to `list` or `export` before downloading. Run `fetch` first.

- **The first `fetch` seems stuck for a minute.**
  That's expected on the very first run — it's downloading the card database and
  waiting politely between requests. Let it finish.

- **"warning: unknown card name…" lines appear.**
  Harmless. A few cards are too new for the card database. The deck still exports
  correctly.

- **"warning: skipping … " for one event.**
  The MTGO website occasionally hiccups on one page. The tool skips it and uses
  the rest. Run `fetch ... --refresh` later to pick it up.

- **`mtgo-deckfinder: command not found`**
  Make sure Step 2 finished, and that you reopened your Terminal after installing
  Rust. You can also run it from the project folder with
  `cargo run --release -- fetch modern` (and the same pattern for other commands).
