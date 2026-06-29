This document describes the core user journey for an exosuit user. It describes the intended flow, both in terms of features that already exist and features that are implied by the flows but still need to be developed. The purpose is to provide a grounding for agents helping to prioritize and build exosuit with Yehuda (the user).

## And So it Begins: An Idea Takes Shape

### The First Session

> This session should fit inside of a single context window, so that the agent provides maximum help in serving as a sounding board and thinking partner. Once compaction has occurred, the conversation will feel disjointed to the user, and the agent will be less effective at helping the user clarify their thinking.

The user starts a new exosuit repository with an idea in mind.

The first step is for the user to discuss the project with Copilot, discussing the idea and the project's goals. This should be a few rounds of conversation to make sure that the user understands their own idea. These first conversations are less about documenting the idea for posterity and more about helping the user clarify their own thinking. The user should be able to ask questions, challenge assumptions, and explore different angles of the idea with Copilot.

Once the user understands what they're trying to build, the agent should help the user decide on some basic parameters for the project. The key at this stage is to start documenting things about the project that are different from the default assumptions of the agent.

> Definition: Parameters that differ from the agent's default assumptions and apply broadly to the project are called _"axioms"_. They are the foundational assumptions of the project, and this is the stage where the user and agent will start to identify and document them.

This process will start orienting the user to the exosuit system.

> [!NOTE]
> Since this is the first way that the user starts to see the way that their thoughts get documented in the "system", it's important that axioms get visualized right away.

## TODO

### Defining the Stack

- core technologies
- for each technology:
  - validation: set up exohook and structure validation so the problems pane is a useful signal
  - decide on a logging strategy that can serve usefully as a feedback loop for the agent (NOTE: we might want something like exohook for logging that helps nudge people towards the right logging strategy)
  - this is where you'd decide on a linting and type feedback strategy: in general, we prefer stronger type discipline with aggressive lints, but _only_ if we know that the tool or types/lints selected produce output that clearly communicates what the user should do
  - testing strategy that will effectively provide a good feedback loop -- this is
  - if the user is building a web app, setting up modern expectations (e.g. playwright, chrome dev tool MCP, etc.) that will both create e2e regression testing and the ability for the agent to interact effectively with the output
    - this is _relatively_ straight forward for CLI tools and libraries, and requires more intention for web apps
    - web apps may be another place where we need to do exohook-like design to give the agent a tool for interacting with a CDP where the app is running, because there's too much friction in the current workflows for it to feel like a useful approach for most agents (there may be something useful to steal from antigravity)
  - e2e testing is very important at this phase -- it's early enough to set a strong foundation, and will ensure that the early code works end to end and powers the steel threads, and also doesn't regress during refactors or rearchitecture
- it's important that the agent help the user towards _modern_ assumptions, _despite the fact_ that the agent's training data is full of outdated assumptions, workarounds, and frustrations by users in earlier times trying to use a more modern stack. We need the whole feedback loop to be as useful to the agent as possible, and the more unnecessary workarounds we use, and the more we use combinations of techniques that are in unnecessary tension with each other, the more the agent will get confused.
- this step should be _relatively_ quick, and it's also a place where we could do some work to establish pre-fab instructions, ideas, etc. to help orient people towards best practices. The exact format of those pre-fab packages (i.e. what is an "exosuit skill") is a design process we have yet to do. It likely has something to do with mapping onto vscode.

### Setting up the First Epoch

- start introducing RFCs
- this is probably a place where the user might have ideas on the kind of custom RFC categories they might want, but we can introduce that gently.
- they'll probably also have a burst of ideas that aren't quite ready to go anywhere yet, so it's a good time to introduce the ideas system.
- we'll want to guide them towards creating one or more RFCs that reflect the first epoch of work, and then organize them
- important (and new): an epoch should be aligned with a single steel thread. In other words: what end-to-end improvements are we expecting by the end of the epoch.
  - brainstorm: this implies adding the notion of a "steel thread" to our system, and I'm thinking it might be a concept _like_ RFCs, but which live alongside RFCs (as the flip side of the same coin). I'm thinking maybe it's a "PRD", but "PRD" is very overloaded, both in normal space and in AI workflow space, and I want to limit what has to go in one of these documents to the steel thread. It would be linked/associated with RFCs (maybe it's actually the parent of a collection of RFCs and advancement planning?), but be focused on the expected imporovements.
  - the reason I think this is important is that each of the main "SOAR" concepts needs to fit into the user's own understanding of their own progress, and actually move the ball. We have _decent_ structure for phases, but epochs are kind of free-form right now.
- this process should feel _relatively_ light weight, and orient them around:
  - epochs (the first one is just "bootstrap", so we can more-or-less have the agent set that up for them and from their perspective it's "just there")
  - steel threads (again, at this stage the agent should try to formulate this for the user based on the earlier conversation rather than asking the user for a ton of back-and-forth)
  - RFCs (this may involve a little more back-and-forth but it should feel like the "meat of the matter" to the user)
  - phases and goals, which, at this stage should largely be something the agent sets up for the user based on the RFC conversation
  - ideas for future work
  - this is probably also a phase in which some more axioms will show themselves -- that's great

> [!NOTE]
> This early stage requires somewhat different steering, to help the agent help the user get oriented. I don't think we would structure it as "we're early", but rather try to scale the steering so it naturally behaves differently when there's not much to work with than when there's a lot of material to triage.

### Setting up the First Phase

- this is where the user will start getting oriented with the idea of the "RFC pipeline". They'll see that there are plans in flight and start seeing them move through the phases towards a conclusion.
- they'll enter the first phase, which should already be populated with goals from the previous step
- the agent will perform the "prepare" step
  - the agent should communicate a summary of what the phase is about and how it fits into the broader steel thread (and potentially how it sets up future epochs). But the sidebar _exists_, so this is not about infodumping. It's about taking the info and orienting it in a way that reflects the user's own understanding of their goals _and helps the user experience progress_. This is a place where properly setting up expectations and infrastructure around epochs, phases, goals + RFCs and advancement (and the other "steel thread" document) will help this step communicate momentum and progress.
  - this is also a place where we can help the user feel like "the wheels are on" and they're not just experiencing a sugar high, although this is more important in phase 2 and epoch 2. But we want to set things up so that by the time they finish phase 2 and epoch 2, they feel like they understand why the process is repeatable.
  - this likely should entail _some_ conversation with the user about questions to resolve -- it's very uncommon that you can map an RFC into goals and tasks perfectly without any user interaction.
  - this process should also set up expectations for user validation. The _goal_ is the right granularity for user validation, so once the tasks are fleshed out, the agent should discuss with the user how both the user and agent will know that the goal is complete, and what manual steps might be necessary. this will help the agent give the right instructions to the user later once a goal is complete
  - this is also the right place to identify what validation is necessary from the agent's perspective. for the most part, exohook should already be set up to serve effectively as a feedback loop, but the preparation stage is also a place where the agent has maximum context and can leave notes for itself about expectations and things a future agent (post-compaction) will want to validate.
  - this is also the right place for the agent to note any _cleanup_ steps they will want to do. the "goal" is also the right granularity for cleanup.

> [!NOTE]
> Just like before, this early stage requires somewhat different steering, to help the agent help the user get oriented. That might include things like instructing the agent to tell the user what's going on more directly, or steering that ensures that the agent sets up the core files correctly. This is a fragile point because (a) there's not many patterns in agent-context to build off of, and (b) anything that goes into the agent-context at this point will strongly inform the rest of the project. Once the agent-context starts getting populated, we can get away with less steering and more "let the agent pattern-match on what it already sees" (to a certain extent -- steering is still a critical part of this project).

> [!NOTE]
> The above implies new infrastructure that we haven't built yet, mostly around goals: tagging goals with user validation steps, agent validation steps, cleanup etc. I don't think we want a new field for each of these, but we will want to do a "steel thread" analysis for how to fit it in (together with any other implied new features).

### The First Goal

- The agent will then use a prepare agent to help create a self-contained execution plan. This might surface changes that need to be made to the RFC or implementation to align it with the goals of the RFC. In general, such changes should be discussed with the user, especially if they cause a change in the manual verification steps or even the agent's goal-specific verification steps. Once this step is done, the goal should be updated to fully reflect the validation steps.
- The agent should take the execution plan and do its own review of the plan based on the superior context in the "main thread". It should consider whether there's anything in the execution plan that diverges enough from user expectations to justify a conversation (that should mostly have already been handled, but the process of producing an integrated execution plan might raise new questions, which basically means repeat the previous step until you reach a steady state). This doesn't require a ton of new back-and-forth with the user: for the most part, the user doesn't need to see the execution plan. It's going to instead go to...
- an execute agent, who implements the goal's tasks.
- this is where granular tools that the execute agent uses for bookkeeping are going to help us steer the process of execution
  - we will want the agent to regularly run the "quick" validation steps (check the problems pane, run the exohook "files changed" lane etc)
  - we will also want to be clear that goals start and end fully green, so _any_ problems reported during task execution need to be addressed, not only if they "seem related"
  - this flywheel should eventually result in the execute agent checking the validation proactively
  - this also implies having a single "validate" tool that rolls up the problems pane and the results from exohook. this is _also_ why I want to connect exohook to the test pane: it provides user visibility into the validation process as the agent is working.

> [!NOTE]
> The user should be able to watch validation happen without asking the agent. This is the "shared perception" principle in action: the Test Explorer and Problems pane are surfaces the user already trusts and monitors. When validation runs, the user sees it happen in real time (green checks accumulating, red failures appearing). The user gets an organic sense of how the agent is doing without having to ask. Critically, the agent sees the same signal in the diagnostic channel of tool call responses, building a robust sense of shared reality that is mediated through the IDE surface.

- the execute agent should work through the tasks, and the "task done" call should (a) run the validation and provide the agent with information about failures (so they can fix them before the task is truly done), and (b) provide additional steering about what to do next.
- note that the "task done" call is the place where we can really push the agent, but we likely want an additional "progress" tool that the agent can use to report progress (which would show up in the sidebar so the user could see it), which would also give us another opportunity to provide contextual steering to the agent. Both:
  - steering about things like "hey, you have errors in the problems pane and here they are, use the problems tool to learn more"
  - general instructions we want to issue as reminders during the execution phase (this gives us a way to provide "project instructions" that are context-specific, which is a powerful aspect of our steering system)
    - note that we could also give the user the ability to provide state-specific prompt guidance via prompts.toml, which would be pretty cool! it would mean nailing down what "state-specific" means well enough to serve as a stable API, but I think the work to do that would really improve the quality of our own internal steering and help sharpen our thinking.
- a goal would either:
  - fail to complete because the assumptions that went into its planning were wrong, or new problems were found (this is something we would need to make sure the execute agent new to identify, causing it to stop mashing and abort)
  - surface additional questions for the user to help answer (again, the execute agent would need to know to do this, and we'd need to figure out how to tune the instructions to avoid over-eagerly having this happen)
  - be done, according to the execute agent
- if the goal is done, the execute agent would run the validation via exohook (we'd have a "slower" version of the validation that was still scoped to changed files, etc., but which might include things like running full clippy that would be too slow for inline, interactive use as part of the progress step)
  - the more I think about it the more I think task should likely do this anyway, so this is more of a belt-and-suspenders "make sure everything is green before reporting done", which should likely have already happened when the final task was marked done. maybe we run the full test suite here and just the change-based test suite after tasks? this question reflects a lot about the design space we need to refine.
- a goal is done when all of its tasks are done, and this has a visual representation in the UI. When the goal is marked as hollow green, the rest of this process proceeds (this likely requires additional steering to make it work)
- if the goal is _not done_, the agent will report back to the user and get feedback user (most of the time, but sometimes it might be able to resolve the question directly). If the goal can be continued, it will update the tasks and completion criteria and give the execute agent the feedback and tell it to continue. If the goal cannot be continued, the agent will help the user restructure the phase or possibly even close it out and restructure the epoch (in rare cases).
- if the goal _is_ done, the agent will perform the manual verification steps described _for the agent_. this may result in some iteration with the execute agent, but hopefully they were captured well enough in the instructions for the execute agent that they go well.
  - once _that's_ done, the agent will give the user a summary of what happened (including anything unexpected), and work through the manual validation steps.

> This entire process should _largely_ feel lightweight _to the user_, and the key thing at this point is to make the user feel a _sense of momentum and progress_. This means helping them see progress happening, and giving them a sense of achievement as the goals get complete and they see for themselves that things are working.
>
> This is _always_ important, but at this early stage, it may mean investing a little bit in checkpoints that help the user see that things are working but might be ephemeral. If the user is building a CLI tool, that might involve the agent running the CLI so the user can see the output in the terminal (and teaching the user to click the "show terminal" button next to the shell command to see the output). If it's a library, it could involve investing in an example script that exercises the library and which has output that demonstrates progress. If it's a web app, that could involve having more ephemeral _mockups_ that show a more complete state that's still being iterated on, so the user gets a sense of where things are heading.

> [!IMPORTANT]
> The first goal establishes the feeling of iterative progress that Exosuit is based on. It should both give the user a sense of progress and momentum, and also start imprinting a pattern that they will experience throughout the first phase. It's critical that this first experience is positive and reinforces the right patterns, so we should invest in making sure it goes well. This is also the point where the user starts to see the value of the agent as a thinking partner and project manager, so it's important that the agent does a good job of communicating what's going on and helping the user feel like they're in control of the process, even though they're getting a lot of help from the agent.

The user's first session could end anywhere (mid-goal, between goals, after the first phase, or even during setup). Wherever they stop, they need to feel motivated to come back, and when they do, it should _feel like momentum continuing_. The sidebar is an important bridge: the user can take a look at it when they return and feel oriented. It grounds the kinds of questions they will want to ask the agent, and since the agent perceives the same state, will make that continuation feel natural to both. One observation: we probably want to give the agent a way to tell that the user is returning after a break (probably by exposing the "time since last action" or something like that to the agent via steering when it's relevant -- exactly how to best communicate it requires design work, but I think the improvement to shared perception at the critical "coming back after a break or time away" juncture would pay significant dividends)

### The Rest of the First Phase

- Once the user has approved the goal and it's marked as "solid green" in the phase, the process above repeats in the second goal.
- This is also a place where it may be helpful to visualize progress in the RFC pipeline. We've started work on this, but I don't feel like we've fully nailed where it goes and how it's represented. This user journey is probably a good piece of context for thinking it through better.
- The process is repeated, goal by goal, until the phase is complete.
- This leads to validation of the entire phase (need a bit more detail fleshed out here) and ultimately phase completion and RFC advancement.
- A phase is the unit of commit, so if commits didn't happen earlier (which would be fine, but it's not mandatory), now is the time to commit.
- Committing will also trigger the full set of exosuit hooks, which may result in the creation of an addition cleanup goal (it's important to actually reflect this) to get the commit fully prepared.
- We shouldn't try to reproduce the _entire_ git flow, but we want to be able to steer the agent here, so having LM tools to mark the git flow (and possibly even manage the commit itself, since that interacts with exohook and is full of sources for confusion) would probably be a good idea.
- The phase should end with the creation of a pull request and the process of merging it, which should also be reified as a goal.

> [!IMPORTANT]
> This is a critical juncture in the user's journey with exosuit. They have just experienced repeated, user visible success with the goal flow, and they are about to merge their first PR. It is critical that this goes well, and that the PR process doesn't create overhead that makes them doubt the process. We want to start with a PR so that they experience the entire process as a sequence of repeated steps that gets them where they want to be, and a low-overhead PR process will allow them to see checks running and passing, and give them a sense that the whole thing is a well-oiled machine producing high quality.

### Between Phases

- this is a place where the user and agent can have a more high-level conversation about how the phase went, what they learned, and what they want to do next. This is also a good time to talk about the next epoch and the steel thread for that epoch, and how the next phase fits into it. That's _always_ true, but in this first instance, we should steer the agent to be more explicit about what's going on to help the user understand.
- They will likely be eager to move on to the next phase, so the key here is to help the user see the first phase as progress towards the steel thread in their goal, and then frame the next phase in terms of progress towards the steel thread.
- Pretty much everything in the above still applies. The key with the second phase is to make it feel consistent with the first phase, so they experience the "well oiled machine" and start seeing how it can power indefinite work.
- This is also a place where the user will probably start having new ideas and new things to RFC. That's what the ideas collection is for. The agent should help the user capture their ideas in enough detail to feel confident that they're not going to get lost, then keep making progress towards their goal.

### End of Epoch / Between Epochs

- At the end of the epoch, there should be a retrospective on the epoch as a whole, and then planning for the next epoch. This is where the steel thread document would really come in handy to help structure the conversation and make sure that it stays focused on progress towards the user's goals.
- This is also where the user and agent should triage the ideas into RFCs, or decide to defer them.
- To make this make work mechanically, we likely need to model this "in-between epochs" point as a "chore" phase that lives outside of any epoch. Its purpose is to support triaging ideas into RFCs, and the open RFCs into project plans.
- This is also a period, like the earliest period, where the agent is helping the user organize their own thoughts. It's less important to push the user towards a specific task the agent thinks is important, and more important for the user to feel like they have the project plan and purpose in their head.
- This is _also_ a point where new axioms would naturally surface. They might have surfaced during the previous phases (and it may be worth steering the user and agents towards that in the between-phases periods in the earliest epochs, and easing off as the axioms fill out), but it's almost certainly the case that after a full epoch, there are new axioms to add. The process of adding axioms should feel to the user like they're updating their own understanding of the project, and that the agent is helping them keep track of it, rather than like they're filling out a form for the agent.
- It also helps the user feel like their own priorities are getting documented. Since axioms are, by definition, things that diverge from "default" expectations of the model, undocumented axioms will keep surfacing as things the user expected but the agent did wrong. The user should feel like this process is going to minimize that problem _and_ that adding axioms is the right way to address it when it comes up (this may also be a good thing to steer the agent towards, although it happens in prose, so it may be necessary to capture it in copilot-instructions rather than steering, even though we normally prefer steering when we have a clear context).
- This is a place where the agent's ability to synthesize information from across the project and surface it in a way that helps the user understand their own thinking will be really important.

> [!IMPORTANT]
> This planning process is critical to making the user feel that the momentum they felt so far isn't going to lead to a bunch of abandoned plans, a buildup of never-completed planning documents, or ideas they thought were important but got lost along the way.

### The Second Epoch

- The second epoch should feel like a natural continuation of the first epoch, but with more momentum and more of the "machine" feeling. The user should feel like they understand the process now, and they're just getting more efficient at it.
- The second epoch is also where the user will likely start to feel more comfortable pushing back on the agent and making sure that the plans reflect their own priorities, rather than just going along with what the agent suggests. This is a critical point for the user to feel like they have control over the process.

## RFCs

- "unresolved questions" should be tagged as needing to be resolved as exit criteria for a specific stage
  - phases that aim to advance past that stage must resolve the questions
  - the agent should decide whether they need to be resolved before implementation, or whether implementation should inform the answer
  - the RFC should be updated to integrate the answer, not inline with the open question (so open questions get smaller as the RFC advances)
- RFCs should terminate (i.e. stage 4) in "done" (for project planning) or "canon" (for concepts that persist)
  - canon needs better organization, including an index and a process for "integration" that fits into the flow
  - the stage directories should be updated with something like `stage-1-{description}`, but `canon` should just be its own name without numeric prefix. I'm not sure how to organize `done`.
- the directory structure has been important (and still might be for users, since the RFCs are markdown), but we should get _much_ better at treating the entire agent-context, _including RFCs_ as "database" contents that the agent only ever looks at via the LM tools or CLI. They should look at the contents the same way an app agent might occasionally look at the database to get low level information (i.e. rarely, and it's usually the wrong approach unless you're actively working on the API or trying to report a bug).
- We will need a way to fit the Stage 4 canonization process into the "End of an Epoch" planning process. I think it fits, but there's details to work out.

## Appendix: How it All Fits Together

I wrote this in response to a message Copilot sent to me while we were working on the overall feedback loop.

<me>This is pretty good but I think there's an emphasis I want to make</me>

<assistant quote>The agent doesn't just execute blindly—it reads steering at natural pause points</assistant>

<me>
It's more than just this. The design of the system is so that all interesting project management steps work through `exo` tools, so that we can slip steering messages into organic, natural steps. So you don't have to think "let me check diagnostics", we just have to get you to think "I should report progress" and the result of reporting progress will include (a) contextual steering, and (b) diagnostics. You've probably experienced some amount  of that in this context.

It's also why I've invested in tools like exohook in the first place: they're sensible parts of the workflow that could be motivated in simple terms (that you will follow because the workflow seems sensible and the instructions aren't too complicated), but now you're getting rich, contextual information about what you should do to keep the wheels on without it having to be a giant dump in an instruction file. And the more of these tools we have you interact with organically for normal pieces of the workflow, the more ways we have to keep track of the SOAR/ODM/PER loops, the user's expectations and the state of the project, and therefore the more we can inject useful context at the right time.

This is also why we have things like `exo` tasks (which aren't fully wired up usefully yet): they allow us to take something that has a loose pattern ("npm tasks") and give the user an ergonomic way to say "these tasks are for the LLM" and ultimately "this is how the LLM should experience them".

Exosuit in particular also has an additional benefit: the ability to interact directly with vscode APIs and wrap _them_. So we extent this same philosophy into vscode APIs: link them into the loops so that they're providing information to the agent organically as it steps through normal tools and tasks. This is why we incorporate diagnostics. It's also related to the "shared perception" goal: users have already set up their IDEs to present rich information contextually and usefully, and we can take advantage of that setup to project that user perception into the agent's "consciousness" as part of normal flows. This is also where projecting exohook into the Test Explorer interface is valuable: it turns it into something with an IDE representation that we could then project into the agent using this contextual approach.

This is also why the "VSCode" aspect to exosuit is so important. Users have already invested in configuring their IDE inside of ecosystems that have invested _tremendously_ in exposing information usefully via LSPs and other vscode-native interactions. The problems pane _already_ serves as an aggregation of all kinds of diagnostics that the user has set up and trusts, and this is something we can push people on further with a tool that takes advantage of this setup as the foundation of the way humans and AIs share perception in a coding environment.

Humans trust the surfaces they've customized, in part because they're part of ecosystems and communities that work together to make these surfaces work well.

If people are starting to slack on setting this stuff up effectively because it feels irrelevant to AIs, we're going to tell them _and the wider coding ecosystem_ that now is the time to invest more, not less, by using the investment to drive agent understanding.

> [!NOTE]
> I've seen other tools try to wire up LSPs automatically for the user (I think opencode does this). But that requires figuring out which LSPs are appropriate and how to configure them, in an ecosystem that _already_ has vscode extensions for LSPs and vscode configurations that _already exist_ in peoples' settings.json files. We don't need agent-specific ways to reproduce IDE features, we just need to integrate agents directly with those features and encourage workflows that set up the IDE properly rather than invest god knows how much time rebuilding all of the nuances of the setups, compositions, etc. in terms of a rickety, fast-moving agent-first tooling ecosystem.

This quote from Copilot feels like a good summary to me:

> Exosuit leverages that existing investment rather than asking users to learn new surfaces. The Problems pane, Test Explorer, and diagnostics do double duty: they're the user's familiar tools _and_ the agent's perception channel. We're not duplicating what the user sees for the agent—we're making the user's existing setup work for both.

Ultimately, I suspect that we'll want to control (or wrap) tools like file editing, shell execution, etc. so that we can use _most_ tools as "progress" points where we can think semantically and provide contextual follow-ups, but just working with what we've done so far already gets us quite far. I'm mentioning this not because we want to necessary do anything about that right now, but because it can help explain the overall approach and how we could think about ways to improve leverage. It's not just about improving a specific tool or piece of a flow, but how the entire thing links together into an "exosuit" (maybe too cute, fine ;)) that drives the entire flow holistically.
</me>
