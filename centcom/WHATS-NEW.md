# What's New in Alelyon

Shown by the account menu's "What's New" (src/signin/mod.rs reads this file at build time). Newest first. Each
release is a `## ` heading (a date, then an optional title after " — "), followed by `- ` lines in plain words.

## 2026-10-14 — Tasks side by side

- The tasks you start from suggestion chips now show in a Tasks view, several side by side (the ▦ button above the chat). Each one shows what it is doing; you don't type into a task, you steer it from the main chat.
- When a task has a question, needs your approval or proposes a plan, it shows on the task and as a card in the main chat. Answer it in either place, or just tell the agent in the main chat and it passes your answer on. An approval still asks you to confirm it.
- Ask the agent in the main chat to give a task another instruction or to stop it; each task also has a Stop button.
- Your messages never land in a task's own thread.

## 2026-10-14 — One chat

- Lattice's chat is now one conversation that carries on, instead of a list of chats: it opens where you left off.
- Your earlier chats are still there: Show earlier brings them back above, and Continue this picks one up again.
- Working in, above the chat box, picks the project you are working on; the chat moves to its folder and its latest work.
- A project can hold several folders: add them on its page, then switch between them in Working in.
- When you return to something you worked on earlier, your message goes back to that topic by itself (and a new subject starts a new one); a note above the box says where it went.
- Each topic is kept in brief after every few turns, so the agent knows where your other work stands; choose which model writes these summaries (this PC's or a hosted one) on the Tools page.
- The agent remembers across topics: what you said or decided in earlier conversations comes back when it bears on your message, and the agent can search them itself.
- A task you start from a suggestion chip now runs beside the chat as a topic of its own, and the chat stays where it is.
- Activity, in the side bar, shows everything working now, each task beneath the topic it came from, then every topic to jump to.
- Choose how many tasks run at once on the Tools page: 3 unless you pick another number, or no limit. More at once uses more of your PC, or more of a hosted model's charges.
- When your message belongs to another of your projects, the agent can ask to move the work there. Say Switch and a new topic begins in that project's folder; say Stay here and nothing changes.

## 2026-10-14 — Checklists from agents in Lattice

- An agent you run inside Lattice can now give you a checklist of things to do yourself, like any other agent: it is pinned above the chat's composer, and you check each step there.
- The agent sees which steps you checked, and can wait until you are done. There is nothing to set up.

## 2026-10-14 — Agents' file edits wait for your review

- When an agent you run in Lattice's chat writes a file in the chat's folder through Lattice, the change now waits in the Changes panel, as the built-in agent's do. Nothing reaches the disk until you keep it.
- Each change shows under the turn that made it. Lattice also lets the agent read files as the chat sees them, including changes still waiting for review.
- Files outside the chat's folder are refused, and Ask mode or an untrusted folder still refuse every change.
- An agent can still change files with its own tools, such as its shell. Lattice cannot stop those writes: they are on disk at once, and are listed after the turn as changes already made.

## 2026-10-11 — Friends messages are sealed

- New messages to friends are sealed on this PC before they reach Alelyon, for your friend's devices and yours, so the service keeps them in a form it cannot open. An independent review of this is pending, and there is no forward secrecy yet.
- Sealed messages carry a "sealed" mark. Messages from before, and messages to a friend who has not yet opened Friends on an updated app or the website, are plain text.
- When a friend adds a new device, the chat says so.

## 2026-10-10 — Sinai uses the models you choose

- Where Sinai's own loop is installed, it now uses the models you choose for its eyes, embeddings, voice and hearing: its hearing, embeddings and voice from its next start, and its eyes the next time they open.
- It runs what it can run on this PC: a GGUF file, a speech model file, a voice model file. A vision model needs its vision projector (an mmproj file) beside it; without one, the card says so and Sinai keeps its own eyes. For an endpoint or a cloud model, Sinai keeps its own model for now, and the card says so.
- Change Sinai's embedding model: once that model has been measured for recall, Sinai codes its saved conversations once for it, and switching back needs no coding again. Until it is measured, Sinai finds memories by their words, and the embeddings card says it is measuring the model first.
- Before Sinai's memory changes to keep a code per model, it is backed up beside itself; if the backup cannot be made, the memory stays as it was.
- The voice picker lists Kokoro voice model files (.onnx) from the voice folder, ~/.alelyon/angel/voices, where installing Sinai also links its own voice. Alelyon never downloads them.

## 2026-10-10 — Your status and the friends bar

- Choose your status: Online, Away, Do not disturb, or Appear offline. Pick it in the Friends panel; it stays as you set it until you change it.
- Friends who chose Do not disturb show a red dot.
- A friends bar along the bottom of every page shows your status, who is online and your unread messages; press it to open or close Friends.

## 2026-10-10 — Free cloud models for Sinai's parts

- Each of Sinai's parts can now use a free cloud model on your own account: OpenRouter's free open models for its mind, Groq for its ears, Cloudflare Workers AI for its embeddings and voice, and Cloudflare for its eyes once you turn them on.
- Press "Use a free cloud model" in a part's picker, connect your own account, choose a model and read what will be sent, to whom and the free limit. Nothing is sent until you agree, part by part.
- A cloud part's card names its provider, and Sinai's page shows an "in the cloud" mark while one is used.
- At a free limit the part pauses and says when it is back, even after Alelyon restarts. Alelyon never switches to another provider or pays on its own.
- On Cloudflare, Alelyon counts each call and stops at 9,000 of the 10,000 free Neurons a day, whatever your plan, so it never runs into paid use. Your eyes, embeddings and voice share that count.
- Sinai's eyes on Cloudflare ask you to tick "I accept the Llama 3.2 licence" before you can agree.
- Gemini is offered as a labelled choice only: its free tier uses what you send to improve Google's products.
- In this app the mind's cloud model answers on Sinai's page, and the ears' runs the speech engine. The cloud choices for the eyes, embeddings and voice are kept for Sinai's own loop.

## 2026-10-10 — Research that remembers

- Agents can ask your research archive a question and get what was already worked out first: facts from papers, how ideas relate, and earlier answers, before reading papers again.
- What they learn is kept beside your archive, so the next question on the same ground is quick and costs nothing.
- When nothing has the answer, an agent can ask you for a new gathering; nothing is searched (and no credits are spent) until you approve it.
- Research's "Add to Lattice" gives Lattice's agents the same questions (Lattice asks once before running it).
- The Research hub shows what agents kept, with Forget on each, and the gatherings they asked for: Approve opens one as a new subject, and it starts only when you press Gather.

## 2026-10-10 — Checklists from every agent

- Agents working outside Lattice's chats (in a terminal, or in other apps) can give you checklists too: they appear pinned above the composer with the agent's name, and you tick them off the same way.
- When you finish a list, the agent that asked is told, whichever agent it is.

## 2026-10-10 — Window buttons on the right

- Minimise, maximise and close are back in the window's top right corner.
- Drag the window by the empty space along its top, beside Lattice's tabs or above a page's heading, as well as by the Alelyon mark. Double-click there to maximise or restore.

## 2026-10-10 — Checklists for what you need to do

- When you need to do something yourself, Lattice's agent gives it to you as a checklist pinned above the composer, instead of directions that scroll away.
- Check each step as you go: the agent sees your checks as it works, and when every step is checked it is told, so you do not need to write to it.
- Prefer to tell it yourself? Switch off "Tell the agent when all are done" and press Done when you are.

## 2026-10-10 — More moments for hooks

- Hooks now also run when the agent waits for you, when a helper starts or stops, when earlier turns are left out of what the model sees, after an answer, after a tool fails, when a folder is attached, when Lattice closes, and around Tab completions (a hook can keep a file's text from them).
- Antigravity's hooks are read in its own format.
- Hooks for moments you are typing or away run only once you allow them: the Tools page's Hooks card has Allow.

## 2026-10-10 — /review

- Type /review in the composer for a careful review of your changes: the uncommitted ones by default, `/review branch` for everything since the branch left main, `/review against <branch>`, or `/review <commit>`.
- The agent reads the change and the code around it and lists what would go wrong, most severe first, each with its file and line. It changes nothing.

## 2026-10-10 — Jump from a chat

- Each chat has one Jump menu (⌄ on its header): its changes, its folder and its model; every Lattice tab and view; and every Fleet tab. Type to narrow it, Enter takes the first, Esc closes.

## 2026-10-10 — Choose a model for each of Sinai's parts

- The cards for Sinai's mind, eyes, ears, embeddings and voice, on the Overview and in the Machine list on Sinai's page, are buttons now. Each opens that part's picker, with the part's model, where it runs and its state at the top.
- Choose a GGUF file on this PC or an endpoint you set up; for the ears, a speech model file. The ears use it the next time Alelyon starts the speech engine.
- Where Sinai's own mind is installed, it still answers unless you choose another model. Sinai's eyes stay off until you choose a model for them.
- Each card says what uses its choice. The choices for the eyes, embeddings and voice are kept for Sinai's own loop, which does not read them yet.

## 2026-10-10 — Start the speech engine from the Words page

- Every build of Alelyon can now start the speech engine from the Words page, when `angel-ears.exe` is beside Alelyon's program. The engine is open source.
- When its speech model or its recogniser is not on this PC yet, the page says which file to put where. Alelyon never downloads them.
- The Fleet page now says that it comes only with the official Alelyon app.

## 2026-10-10 — Hooks

- Commands you set can run when Lattice's agent works: before a tool runs (to stop it or change it), after it ran, when you send a message, when the agent stops (to have it go on), and when a chat starts.
- Lattice reads your own hooks, your plugins', and those you already use in Claude Code, Cursor, Gemini CLI and Antigravity, and a trusted folder's.
- Each hook asks you once, showing its exact command; any change asks again. The Tools page lists them all and takes an allowance back.

## 2026-10-10 — Follow the agent, and notes beside the code

- Follow, at the right of the editor's tabs: while it is on, each change the agent makes opens in the editor as its diff, as it makes it. A change made by another chat in the grid opens that chat beside the grid first.
- Notes beside the code: explanations attached to lines, shown in the editor and never written into the file. A mark sits by the first line of each note (gold for the agent's), and the notes are listed under the editor; Notes in the file bar hides or shows them.
- When the agent changes a file, its reason becomes a note on the lines it wrote. Add a note puts your own on the lines you select.
- A note follows its lines when lines above them are added or removed, and says so when its lines change.

## 2026-10-10 — More room at the top

- The window no longer has a bar of its own above the page: minimise, maximise and close are at the top of the left rail, and you drag the window by the Alelyon mark or the rail's empty space.

## 2026-10-10 — Tab completions in the editor

- As you pause typing in a file, a code model suggests what comes next in grey: Tab takes it, Ctrl+Right takes its next word, Esc puts it away.
- Choose the model on the Tools page: one on this PC (free, nothing leaves it) or a code model of a provider you connected. Each shows what 1,000 completions cost, grouped by price so you can filter them.

## 2026-10-10 — Where each chat is, and a tidier Chats list

- Under each chat's title: its stages (Plan, Build, Review, Commit, Pull request), the one it is at in gold, and how far along its task list is.
- The Chats list has a search box, then New, Projects and Tools, then your chats by folder with a + for a new one there. Each folder shows its newest eight, and Show more brings the rest.
- Beside the grid, the chat is for planning: it asks and answers and writes nothing.

## 2026-10-10 — Chats side by side

- Put up to ten chats side by side and watch them work at once: press the grid button on a chat in the Chats list.
- Each one is a full chat panel, joined to the next by a thin gold line: its conversation, its approvals and questions, and its own box, mode and model.
- Beside the grid sits the editor with its tabs, or a chat if you pick Chat above it.
- Answering an approval in one opens that chat on the right first, so every answer goes to the chat that asked.

## 2026-10-09 — Fleet activity

- The Fleet page has an Activity tab: every session, the agents it started and what each is doing now, with their latest words, and a running tape of the newest turns.

## 2026-10-09 — Commit and open pull requests from the chat

- In Agent mode, Lattice's agent can make a branch, commit, push and open a pull request (with GitHub's CLI, when it is installed).
- Each commit and each push asks you first, showing the branch, the message or title, and the files. They run your own git, hooks included.

## 2026-10-09 — Undo what Claude Code or Codex changed

- In a chat with a folder, Claude Code and Codex now work in that folder. After each of their turns, the files they changed (by editing or by a command) are listed in the Changes panel, and a note says which.
- Undo puts a file back the way it was before the turn, after your review; Keep just acknowledges it.

## 2026-10-09 — See Claude Code's and Codex's edits before they happen

- When Claude Code or Codex asks to change a file, Lattice's question now shows the change itself: the lines it removes and adds. Nothing is written unless you allow it.
- Codex now asks before every edit too.

## 2026-10-09 — Choose Claude Code's and Codex's models

- On Lattice's Tools page, each installed agent has Choose model…: pick any model it offers (Codex with its reasoning level), or its own default. New chats with that agent use it.

## 2026-10-09 — Sinai's research findings

- When you hand a gap to Sinai, it now reads the gap and its papers from your research archive, looks on the web for newer work that may already close it, and decides whether the gap is real.
- Its verdict, the reason, the next step it suggests and its sources are kept beside the gap on the Research page.

## 2026-10-09 — Helpers that edit

- In Agent mode, a helper can now make changes too. Like the agent's own, they wait in the Changes panel for your review, and nothing changes on disk until you keep them.

## 2026-10-09 — Sinai works on research gaps

- Every gap on the Research page has "Work on this with Sinai": one click sends Sinai the gap, why it looks like one and the papers it rests on, and opens Sinai's page so you can watch it start.
- Sinai is asked to read the papers, look for newer work that already closes the gap, and tell you whether it is real and what to try next.
- A gap you handed over is marked, with when, so you can ask again later.

## 2026-10-09 — The agent remembers

- Lattice's agent can keep short notes about a folder, such as how to build and test it or a convention you asked for, and every new chat in that folder starts with them.
- See them on Lattice's Tools page under "What the agent remembers", and forget any you don't want kept.

## 2026-10-09 — Helpers

- Lattice's agent can send helpers to look into a question in your folder on their own, several at once. A helper only reads; its answer comes back to the agent, so long searches don't crowd your chat.

## 2026-10-09 — The agent reads pages and searches the web

- With its browser switched on, Lattice's agent can read a web page as text, without a screenshot, and search the web (Bing) to find pages.

## 2026-10-09 — The sign-in screen's gold, on every page

- Panels, menus and cards are edged with a fine gold line, and a primary button is a bar of polished gold, as on the sign-in screen.
- Buttons and tabs are dark behind a gold edge that brightens under the pointer; the chosen tab is washed gold, and text boxes turn gold when you type in them.
- Each page's title is set larger over a short gold rule, and the names of its parts are in small, spaced gold capitals.

## 2026-10-09 — Background commands

- Lattice's agent can start a command that keeps running, such as a dev server or a watcher, and check on it later. You approve it as any command; the card says it keeps running.
- Its card shows "background" and a Stop button while it runs. Closing Lattice or archiving the chat ends it.

## 2026-10-09 — Hosted open models

- Models… now lists services that run open models for you: OpenRouter, Hugging Face, Together, Groq, Fireworks, DeepInfra and Cerebras.
- Sign in with OpenRouter in your browser, or paste a key for the others (Get a key opens the right page). Then browse their open models and choose one for Lattice's chat or Sinai.

## 2026-10-09 — Settings over any page

- Settings now open as a panel over the page you are on, like Claude Code's, so everything behind it stays in view. Open them from the account menu.
- Search finds a setting by what it does. Esc or a click outside closes the panel.

## 2026-10-09 — Bring your own model

- Models… on the Sinai page lists the models this PC can use: GGUF files in your models folder and the OpenAI-compatible endpoints you set up, each saying whether it is ready.
- Add a GGUF file… links a model you already have into the models folder (nothing is copied or downloaded). Add an endpoint… takes an address, a model name and, if it needs one, a key, which goes to Windows Credential Manager and is not shown again.
- One click makes a model the one Sinai's page or Lattice's chat uses. Without Sinai's mind installed, Sinai's page talks to your model, its face speaking the answer, and says which model is answering.

## 2026-10-09 — Claude Code and Codex in Lattice

- Lattice's chat can now use Claude Code and Codex on your own accounts: your Claude subscription and your ChatGPT plan. Install them on the Tools page, then pick one in the model list.
- They work in the chat's folder with their own tools. Whatever they ask to do, Lattice asks you first.

## 2026-10-09 — Research: sharper subjects

- In a large subject, papers that only a couple of others cite, and that talk about something else, are kept "on the edge": they stay in the subject under their own filter, and no longer crowd its map, threads and ideas.
- The same paper listed twice with a tag such as "(Abstract Only)" is now one paper.

## 2026-10-09 — Mention files with @

- Type @ in the box you type in to pick a file of the open folder. Lattice's agent is given the files you mention along with your message, and a note says which ones it got.

## 2026-10-09 — Images in your messages

- Attach a screenshot or another picture to a message for Lattice's agent: press Image beside the box you type in, or drop a PNG or JPEG file on Lattice. Up to four, each up to 5 MB.
- The agent's model sees them with your words. When the model runs off this computer, Lattice asks each time before sending images, since it cannot check a picture for passwords or keys.

## 2026-10-09 — Plan first

- In Ask mode, Lattice's agent can look around and then propose a plan for the change you asked for. Nothing changes until you approve it.
- Approve carries the plan out in the same chat, in Agent mode. Keep planning sets it aside so you can ask for changes.

## 2026-10-09 — Session inspector

- The Fleet page has a Session inspector tab: pick any session to see what it is working on, what it has claimed, how many tokens it has spent, and where it overlaps with other sessions.

## 2026-10-09 — Flow

- The Fleet page has a Flow tab: the project's work as a production line, showing where along the way from claimed to landed the work has piled up and which parts of the project the most work depends on.

## 2026-10-09 — Plan

- The Fleet page has a Plan tab: what to work on next from the project's own documents, why it comes first, what is blocked and on what, and how far each item is from done.

## 2026-10-09 — The agent's to-do list

- On longer work, Lattice's agent keeps a list of its steps above the box you type in, so you can see what it has done and what it is doing now.

## 2026-10-09 — Artifacts

- Lattice's agent can save a document beside the chat, such as a plan, a report or a page, and update it as it works. Each one shows as a card in the chat with its version.
- Open shows it in a tab of its own: Markdown formatted (or as its source), every version it has had, and Copy.
- Preview opens a page or a picture the agent made in a separate browser that has no internet access and none of your sign-ins.

## 2026-10-09 — Distribution

- The Fleet page has a Distribution tab: which models did which jobs in this project and how long those jobs ran, over all time or the last week, month or quarter.

## 2026-10-09 — Declared space

- The Fleet page has a Declared space tab: the fleet's layers, the kinds of work each one handles, and which models have done that work in this project.

## 2026-10-09 — Run history

- The Fleet page has a Run history tab: what each model has done in this project, how much of its work landed, and its most recent runs. Models are listed by how much they did, never ranked by score.

## 2026-10-09 — Branches

- The Fleet page has a Branches tab: every branch of the project and what it changed, the shared lanes and whether each is free, and the files many branches have touched.

## 2026-10-09 — Channels

- The Fleet page has a Channels tab: the sessions' chat rooms, with what you have not read yet, and each conversation with its replies under the message they answer.

## 2026-10-09 — Contested areas

- The Fleet page has a Contested areas tab: the parts of the project more than one session is working on or has claimed, and how many tokens are riding on each.

## 2026-10-09 — Spend

- The Fleet page has a Spend tab: how many tokens each Claude Code session has spent, the largest first, with the share each holds and when the output was spent. Tokens only, since there are no prices to go by.

## 2026-10-09 — Waiting on you

- The Fleet page has a Waiting on you tab: every Claude Code session on this computer and which of them is waiting for you, the ones asking you a question first. A session that said it is blocked shows what it said beside it.

## 2026-10-09 — The official build

- Alelyon comes in two builds. The official one also replays a receipt's number, width and budget on the project's own engine; the open one checks the signature, the data and the records, says plainly that the replay engine is not in it, and never marks a receipt verified.
- In the open build, Sinai's face and page are there but its mind is not: the page says so instead of waiting for it. The speech engine is read when it is running, and voice enrolment is in the official build.

## 2026-10-08 — Research keeps up

- Gathering papers runs in the background on its own and saves after every round: closing Alelyon, or Alelyon closing unexpectedly, no longer loses a run, and reopening the Research page shows it still going.
- The Research page no longer keeps redrawing while a run goes, which could make Alelyon freeze or close on some graphics drivers.
- Watch a subject daily or weekly: while Alelyon is open, it adds what is new to the subject on its own, using only a few hundred OpenAlex credits each time. Watching is off until you switch it on.
- Update now brings a subject up to date at once.
- Your hub shows what the updates brought: new papers first, then a count of older papers they found for the first time.

## 2026-10-08 — Live data

- Pages update themselves when what they show changes: the services, the fleet, the databases, the transcript library, the voiceprint, the sign-in service and the GPU job records. The Check again, Refresh, Look again and Read now buttons are gone.
- A small mark on each says it is live and when it last changed, or why its source cannot be read.

## 2026-10-08 — Plugins

- Add a plugin from its folder on the Tools page: a folder in Claude Code's plugin layout, such as Alelyon's own. While it is on, its commands (/plugin:name) and skills join yours in every chat.
- A plugin's MCP servers start only if you copy them into your settings, and each asks you first. Its hooks never run.
- Switch a plugin off, or take it off the list; its folder is never deleted.

## 2026-10-08 — A quieter window

- A page that is not changing is no longer drawn again and again: the Overview's service check no longer flickers its button every ten seconds, and spinners turn only where you can see them.
- Long lists in the Lattice IDE (the Explorer and search results) build only the rows on screen, so a large folder scrolls smoothly.

## 2026-10-08 — Suggested tasks

- While it works, Lattice's agent can suggest a separate task it noticed on the way, such as a stale README or a missing test. It shows as a card in the chat, and nothing happens until you choose.
- Start opens a new chat in the same folder that begins with the task's prompt; Show prompt lets you read it first. Dismiss sets the task aside.

## 2026-10-08 — Web pages in Lattice's browser

- The account menu's and Settings' web pages (your account details and security, creating an account, help signing in, the Terms and the Privacy Notice) now open in Lattice's own browser, each in a new tab, and the agent keeps its own page.
- Prefer your own browser? Choose it in Settings, under This PC, or use the small arrow beside any link to open that page there once.
- Signing in with Google, GitHub or another provider still uses your own browser.

## 2026-10-08 — Research hub, maps and gaps

- The Research page now opens on your hub: ideas to pursue, papers to read next, the newest work, and subjects you could start, ranked toward what you search, open and save. What it learns from stays on this PC.
- Each subject has a map of how its papers connect: threads of related work in colour, the most influential papers larger. Click a paper to see what it cites, what cites it, and its closest neighbours.
- A Gaps tab shows where a subject may have open questions: lines of work that rarely cite each other, older ideas recent work stopped citing, combinations nobody has tried, and important papers the subject is missing. Finding gaps uses no OpenAlex credits.
- AI models and agents can use the same archive: copy the MCP setup from the Research page into Lattice's Tools page or another MCP client.

## 2026-10-08 — Research

- A Research page gathers every paper on a subject: it searches OpenAlex and arXiv, then follows citations backward and forward until nothing new turns up, so older papers that use different words are found too.
- Each paper shows why it is included: it matches your concepts, or the subject's own papers cite it. A filter shows the papers found only through citations.
- OpenAlex needs a free key for real use. The page has a button that opens OpenAlex's key page; paste the key in, and Alelyon keeps it in Windows Credential Manager and shows how much of today's budget is left.
- A run shows its progress and can be stopped. When it finishes, the page says whether it was complete and estimates how many relevant papers may still be missing.

## 2026-10-08 — Commands and skills

- Type / at the start of the chat's composer to run a command: a saved prompt from your own commands folder, or from a trusted folder's .claude/commands, .cursor/commands or .lattice/commands. Its text goes into the composer, ready for you to read and send.
- Skills are folders with a SKILL.md that tell the agent how to do a kind of task. The agent sees their names in every chat and reads one when a task calls for it. Yours go in your skills folder; a trusted folder's .claude/skills and .lattice/skills count too.
- The Tools page lists your skills and commands, and where to add them.

## 2026-10-08 — Projects

- Projects group your chats in the Lattice IDE. Pick one in Chats to see its chats; a new chat then joins it, and the open chat can be added to it.
- A project's page holds instructions for the agent and reference files (text files on this PC) that go with every turn of its chats, a folder it can open, and the model a new chat in it starts with.
- A project is archived, never deleted, and keeps its chats.

## 2026-10-08 — Auto mode

- Auto mode lets the agent use your whole desktop in Agent mode: it sees your screen and uses your mouse and keyboard in any program. Switch it on in the Tools page; Alelyon asks you to confirm first.
- It posts, sends, edits and deletes without asking. A purchase, a payment, and a sign-in, security or account change still ask you.
- Ctrl+Alt+End stops the agent at once, from any program.
- It never acts on Alelyon itself, a password manager or Windows' sign-in prompts, and never types a password.

## 2026-10-08 — Connections

- A Connections page in the Lattice IDE lists Google, Amazon, Amazon Web Services, Instagram, X, Facebook, LinkedIn, GitHub, Outlook and Reddit. Sign in opens the agent's browser at the service's sign-in page, and you sign in yourself.
- A chat with no folder can now use them: in Agent mode the agent uses its browser and your own MCP servers, and it never touches files or runs commands there.
- As everywhere, posting, messaging, buying, deleting and account changes ask you first.

## 2026-10-08 — Local models get tools

- The local model (Auto and Local) is checked once, at its first Agent-mode turn, for whether it calls tools well. If it does, it gets the agent's tools, the browser included when it can see; if not, it answers in plain text, and the chat says so.

## 2026-10-08 — The agent's browser

- The agent has a web browser of its own (Microsoft Edge, with its own profile). Switch it on in the Tools page; until then nothing starts it.
- In Agent mode, with a model that can see images, the agent opens websites, looks at them, and clicks, types and scrolls as a person does.
- Posting, messaging, buying, deleting and account or consent changes ask you first: a card in the chat, then a confirmation. A click that looks like one of them but was called harmless is refused until the agent says what it is.
- It never types a password or card details: open its window with Show and sign in to a site yourself. Your sign-ins stay in its profile on this PC.
- Its screenshots go to the model you chose; with a model off this PC, they leave it.

## 2026-10-08 — Tools and MCP servers

- A Tools page in the Lattice IDE lists the agent's own tools and the MCP servers it can use.
- Add an MCP server by pasting the entry its instructions give; a server that is a web address is not supported, because it would send your data off this PC.
- A server runs only after you enable it, and its tools join only Agent mode in a folder you trust. Each call asks you first, unless you choose to always allow that tool.
- Start, stop or switch off a server, switch single tools on or off, and see its last log lines.
- Projects can bring their own servers in .lattice/mcp.json, .mcp.json or .cursor/mcp.json; each still waits for you to enable it.
- The sign-in screen's moving background uses far less of your graphics card: it slows down when nothing stirs it, stops while Alelyon is not the focused window, and a Motion button in its corner (or Settings, Appearance) switches it to a still picture that costs nothing; Alelyon remembers your choice.

## 2026-10-08 — Friends

- Friends: the smiley in the rail opens your friends list — who is online, requests to accept, and one-to-one chat.
- Add a friend by their exact username. Choose your own username the first time you open Friends.
- The sign-in screen is a living scene: a carbon-fibre plain under rolling gold fog, which your cursor stirs. Without a graphics card it shows a still gradient instead.
- The window has no Windows frame any more: its title bar, buttons and edges are Alelyon's own, and the sign-in form sits on the background.

## 2026-10-08 — Lattice IDE

- The Lattice page's Chat is now a Chat and IDE: open a folder, browse and search its files, edit and save them, with the agent beside your code.
- Your own edits are saved with Ctrl+S, and never over a file that changed on disk since you opened it; files that change what tools and builds do ask first.
- The agent's changes wait for your review: keep or undo each part, each file, or all of them.
- A terminal in the bottom panel (Ctrl+J): your own PowerShell, in the folder you have open. Only what you type or paste runs there; the agent cannot type into it.
- Find and replace in a file (Ctrl+F, Ctrl+H), go to a line (Ctrl+G), and search the whole folder (Ctrl+Shift+F).
- Ctrl+K: select some lines and say what should change; the agent proposes the edit for you to review.
- Enter keeps your indentation, Tab indents the way the file already does, Ctrl+/ comments lines out and back in, and Alt+Up and Alt+Down move lines.

## 2026-10-07 — Accounts

- Sign in to your Alelyon account, or keep using Alelyon offline. Google, Hugging Face and LinkedIn sign-in open in your browser.
- Sign in on this PC by scanning a QR code with a phone that is already signed in.
- "Stay signed in" keeps your session sealed to your Windows user on this PC.
- A banner at the top tells you about scheduled maintenance before it happens.
- The account menu has your account pages, settings, this list, sign out and exit.
- Settings (Account in the rail) shows who is signed in, whether this PC keeps your session (and forgets it on request), and the sign-in service's state, ways to sign in and notices.
- A new account made through Google, Hugging Face or LinkedIn asks you to agree to the Terms and the Privacy Notice first.

## 2026-10-07 — Sinai

- Voice enrolment in the Machine: see the voice Sinai knows as yours, record it again, or erase it.
- Sinai's eyes have their own place in the Machine.
- The Machine has a memory map and an observatory.
- The appearance creator turns Sinai's model only while you hold the mouse button down.
- Alelyon opens without a separate console window.
