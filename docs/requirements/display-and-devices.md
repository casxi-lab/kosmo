# Requirements on krepi: display and devices

This document lists what kosmo needs from krepi for the display path on the
device: the display and input device services that kosmo's display server holds,
the shared buffers and the transport of the display protocol, the supervisor
that starts the display server and stops jobs, application registration, memory
pressure, restricted modes, traces from input to presentation, pointing devices,
Linux compatibility for the compatibility service, the build of kosmo's crates
for the device, a system configuration area, the source of role grants, and
kosmo's part in krepi's acceptance. It is written for people who build kosmo or
krepi.

The numbered requirements are kosmo's needs for the display path on krepi. Each
states what kosmo needs from krepi and how kosmo uses it; how krepi provides it
is krepi's decision, and krepi can accept or reject each requirement on its own.
What the public documents of
[krepi](https://github.com/casxi-lab/krepi/tree/e099b40) already settle, at
commit `e099b40`, is not asked for again but cited where it applies: among other
things, the display and input device services and the drivers that run them, the
allocation of scan-out buffers by the display device service, messages of two
64-bit words with a one-shot reply capability for each call, at most one
capability with a call, notifications and wait sets, and the last frame staying
on the display when its holder dies. What kosmo's display server assumes of its
ports is stated in the [device ports](../architecture.md#device-ports) and the
[transport rules](../architecture.md#transport-rules) of the architecture; the
requirements link these sections instead of restating them.

A requirement that krepi rejects does not change kosmo's design in this
document; kosmo's design changes only through its architecture, whose [display
stack](../architecture.md#display-stack) section states which port details are
provisional and when they are compared with krepi's interfaces.

## Display device service

1. **Output enumeration.** krepi's [device
   rules](https://github.com/casxi-lab/krepi/blob/e099b40/docs/architecture.md#devices-and-dma)
   state that the output enumeration of a display device service gives the
   transforms that the scan-out hardware of each output can apply. kosmo also
   needs it to give the instance's epoch; the instance's capabilities, that is,
   whether it sends vertical blanking events, whether it sends releases of
   scan-out buffers, how many scan-out buffers each output can allocate and
   whether it has panel power; and for each output an identifier, its current
   mode as width, height and refresh rate in millihertz, and the formats it can
   scan out, a subset of kosmo's four [pixel
   formats](../architecture.md#pixel-formats-and-buffers). The display server
   reads the enumeration when it starts and after each epoch change, composes
   each output in its size and format, and chooses its fallbacks from the
   capabilities, as the [display device
   port](../architecture.md#display-device-port) states.
2. **The axis of mirroring.** The same rules state that the HVS can rotate by
   180 degrees and mirror, and that RP1 DSI can apply no transform. kosmo needs
   krepi to state the axis across which scan-out hardware such as the HVS
   mirrors, so that the transforms of every output are a subset of the eight
   symmetries of the plane, the identity always included. The display server
   compares the orientation that it chooses with that subset and performs any
   other orientation while it composes.
3. **Scan-out buffers.** krepi's device rules state that the display device
   service allocates every buffer that a display scans out and grants mappings
   of it to its holders, and that a holder never supplies the pages that a
   device scans out. kosmo needs the service to allocate scan-out buffers for a
   holder by output and scan-out format, in the size of the output's current
   mode, and to give the handle and the stride of each; every output can
   allocate at least one, and the capabilities report how many. How the holder
   writes a rectangle that is already in the scan-out format into a scan-out
   buffer, through a mapping or by a copy, is krepi's decision. The display
   server writes the damaged rectangles of its composed result into a scan-out
   buffer before it commits the buffer.
4. **Atomic commits with update regions.** The display device service commits
   one scan-out buffer to an output together with a list of update regions, and
   accepts any list of up to 16. A commit takes effect atomically per output:
   the scan-out buffer and its update regions take effect together when the
   commit completes, and when a commit changes the scan-out buffer, nothing of
   the new buffer is shown before the completion. An output has at most one
   commit that has not completed; another commit returns busy, which is
   backpressure and not an error. Commits across outputs need not be atomic. The
   display server merges its damage into at most 16 regions and waits for the
   completion of an output's commit before it commits that output again.
5. **Completion, vertical blanking and release events.** The display device
   service sends three events: the completion of a commit with the commit's
   identifier, a vertical blanking event with its output, and the release of a
   scan-out buffer. Each carries a time that converts into the holder's
   monotonic clock domain (requirement 13). krepi's device rules state that
   where a device raises no vertical blanking interrupt, the service generates
   vertical blanking events from a timer at the refresh rate of the mode; kosmo
   needs these to be the same events as those of hardware vertical blanking, so
   that the display server times its frames from both in one way.
6. **The time source of a completion.** A completion event states where its time
   comes from: a time that the scan-out hardware gives, or a time that software
   derives. kosmo's presentation feedback and its traces from input to
   presentation use it to tell measured times from derived ones. The display
   device port has no such field yet; it will be added as a compatible revision.
7. **Panel power.** The display device service has an operation that sets the
   panel power of an output to on, standby or off. An instance without panel
   power reports this in its capabilities, and the holder then never calls the
   operation. kosmo holds the brightness and screen-off policy and uses the
   operation to turn panels off; the backlight mechanism belongs to krepi.
8. **Losing and regaining scan-out.** krepi's [protection
   domains](https://github.com/casxi-lab/krepi/blob/e099b40/docs/architecture.md#protection-domains)
   state that the holders of a display driver reconnect when it restarts, and
   its device rules that a holder that dies loses only its mappings. kosmo needs
   two events for the holder: holder lost, when a second holder has taken over
   scan-out or the service no longer serves this holder; and epoch changed, when
   the service hands scan-out back with a new epoch or gives it to the holder
   after restarting. After an epoch change every handle of the old epoch is
   invalid and no event of the old epoch is delivered, and a write or a commit
   while the holder does not hold the instance returns not holder. Who holds the
   instance and when it is handed back are krepi's rules. The display server
   follows the [holder handover](../architecture.md#holder-handover).
9. **The minimal frame-buffer subset.** The minimal frame-buffer subset that
   krepi implements first has the first six items of the profile in [minimal
   frame-buffer subset](../architecture.md#minimal-frame-buffer-subset): output
   enumeration with the capabilities, at least one scan-out buffer per output,
   writing into scan-out buffers, commits with update regions, completion
   events, and the holder-lost and epoch-change events. It may lack the other
   five, vertical blanking events, release events, a second scan-out buffer per
   output, panel power and hardware transforms other than the identity, and the
   instance's capabilities report what it lacks. The epoch events are in the
   subset although it has a single holder; that section states why. The display
   server chooses its fallbacks from the capabilities, never by waiting for an
   event that does not come.
10. **Modes and hardware transforms.** The display device service gives the list
    of modes that each output can use, switches the mode of an output when the
    holder requests it, and sets a transform that the scan-out hardware of an
    output can apply when the holder requests it; the driver carries out clocks,
    scan-out and mode switching. The display server holds the policy for mode
    and orientation, as the [display device
    port](../architecture.md#display-device-port) states, and will add the mode
    list and the request for hardware transforms to the port as compatible
    revisions.
11. **Output hot plugging.** The display device service tells the holder when an
    output is connected or disconnected and gives the new output enumeration.
    The display server then follows the new enumeration, as it does after an
    epoch change. This is separate from the hot plugging of input devices
    (requirement 18).

## Input device service

12. **HID usages as reported.** The records of an input device service give each
    key's HID usage page and usage ID, 16 bits each, as the device reports them,
    keys on vendor-defined pages included: they are not changed, not folded into
    the keyboard page `0x07` and not dropped. The display server's key maps are
    keyed by usage and its [device record](../architecture.md#device-record)
    holds page and ID unchanged, so a key of a vendor-defined page gets its
    meaning from the key map like any other key.
13. **One clock domain.** krepi's device rules state that the class driver of a
    USB HID device takes reports with the time at which their interrupt was
    taken, but do not state how that time reaches the driver. kosmo needs the
    interrupt timestamps of input records, the times of display device events
    and the monotonic time that the holder reads to convert into one monotonic
    clock domain, and krepi to state which clock the timestamps use and what the
    conversion rests on. krepi's integration converts them behind the ports, as
    the [port structure](../architecture.md#port-structure) states, and the
    display server times repeat, long press and frames in that domain.
14. **State query.** An input device service answers a query with the set of
    usages that the instance currently reports pressed; kosmo's side holds at
    most 32. After a loss mark the display server queries the state to
    resynchronize, as the [input device
    port](../architecture.md#input-device-port) states.
15. **A bounded record queue.** The record queue of an input device service is
    bounded, and krepi states its capacity. When it is full, records are
    dropped, a loss mark stands at their position, and the driver never blocks.
    The display server drops the instance's records up to the end of the packet
    and resynchronizes with the state query (requirement 14).
16. **The description of an instance.** krepi's protection domains state that
    the driver of each USB HID device, I2C keyboard or GPIO key runs its input
    device service, with the reserved stop key registered to it. kosmo needs
    each input device service to be an instance with its own epoch and
    description, and the description to give the class, a keyboard or a device
    with keys only; a layout hint that chooses the key map and tells apart
    keyboards that send the same usages; and the reserved keys registered in the
    instance's configuration, with usage and kind, at first only the stop key.
    The display server chooses each instance's key map by its layout hint and
    builds its reserved key table from the registered keys, as the [key
    maps](../architecture.md#key-maps) and [sessions, roles and
    placement](../architecture.md#sessions-roles-and-placement) state.
17. **Epoch change and removal of an input instance.** krepi's protection
    domains state that only the devices that a failing input driver drives stop
    delivering events. kosmo needs the input device service to tell the holder
    when an instance's epoch changes, because its driver restarted, and when the
    instance is removed; after an epoch change no record of the old epoch is
    delivered. The display server then synthesizes releases for the keys pressed
    on that instance.
18. **Input hot plugging.** When an input device is connected while the system
    runs, a new input device service instance is given to the holder. The [input
    device port](../architecture.md#input-device-port) will add instances as a
    compatible revision.
19. **Keyboard lights.** The holder can set the lights of a keyboard. The
    display server lights them from kosmo's [layer
    state](../architecture.md#layer-keys-and-layer-state), for example to show a
    locked layer. The input device port has no lights yet.

## Shared buffers

20. **Read-only grants of shared memory.** krepi's [capability
    rules](https://github.com/casxi-lab/krepi/blob/e099b40/docs/domains/objects-capabilities/capabilities/contract.md#derivation-and-badges)
    state that derivation, copy and transfer never add a right, and its [memory
    rules](https://github.com/casxi-lab/krepi/blob/e099b40/docs/architecture.md#address-spaces-and-vm-objects)
    that shared objects are not provided at first. kosmo needs a client to be
    able to grant a shared memory object to the display server read-only, so
    that the capability that the server gets cannot write the memory. The server
    only copies client buffers, as [buffer sealing and private
    copies](../architecture.md#buffer-sealing-and-private-copies) states. The
    object is the one that a call carries in requirement 60.
21. **Revocation during a copy.** While the display server copies a committed
    buffer, a revocation by the client or the client's death causes no fatal
    fault in the server; at most that copy fails. How it does so is krepi's
    decision. Ways to do it include passing the read right to the server at each
    commit and back at the release; a copy that recovers through a user fault
    handler, which krepi's
    [upcalls](https://github.com/casxi-lab/krepi/blob/e099b40/docs/architecture.md#futexes-and-upcalls)
    already deliver; and deferring the revocation of a grant that is being read
    until the server gives it back. kosmo's side leans to the last, because the
    server reads a buffer only during one copy and the first would add a
    capability transfer to every commit. When a copy fails, the server closes
    that client's session with a buffer fault and carries on.

## Application registration

22. **Display name.** The application registration carries the application's
    display name, in several languages, which the shell's launcher and the
    documents that host its regions show.
23. **Icon.** The application registration carries the application's icon, which
    the launcher shows.
24. **Entry.** The application registration carries the entry through which the
    shell starts the application.
25. **Default opener.** The application registration states the document types
    that the application opens by default, so that file browsing finds the
    default opener of a file by its type.
26. **Region types.** The application registration carries the region types that
    the application provides, so that a document can place the application's
    regions on a page.
27. **Name bitmap.** The application registration also carries a bitmap of the
    application's name that a trusted party renders in advance and keeps with
    the registration. The display server's [trusted
    presentation](../architecture.md#trusted-presentation) copies it unchanged
    and lays out no text; on a host a stand-in registration gives it.

## Stop key and foreground session

28. **The stop key's press at once.** krepi's input driver passes the press of
    the reserved stop key to the holder at once and never holds it back to judge
    a long press. The display server judges a short press as soon as it reads
    the press, as the [stop key](../architecture.md#stop-key) states.
29. **Events after a long press.** krepi's device rules state that the driver
    that recognizes the long press of the reserved stop key reports it straight
    to the root. kosmo needs krepi to state whether, after that, the driver
    still passes this key's presses and releases to the holder. Either answer
    works with the display server's part of the [stop
    key](../architecture.md#stop-key), which ignores releases and treats a
    further press as a new press, but it has to be stated.
30. **A restricted stop capability per session.** krepi grants the display
    server, for each session, a restricted stop capability that can stop only
    that session. The server uses it to ask, through the control endpoint of
    krepi's supervisor, that the jobs in focus of the foreground session be
    stopped, as the [stop-request port](../architecture.md#stop-request-port)
    states. How the server gets the capability is krepi's decision; the server
    holds no general power to stop any session.
31. **Cancelling, then ending workers.** When the supervisor receives a stop
    request, it first asks the job service to cancel the session's jobs in focus
    cooperatively, and past a grace period ends the session's isolated worker
    processes. Who carries this out and how long the grace period is are krepi's
    decisions.
32. **Merging stops.** The supervisor merges concurrent stops of one session,
    the request of a short press and the emergency path of a long press, treats
    an emergency stop as a superset of cancelling, and publishes one final state
    for each job. A long press first sends one short-press request through the
    display server, so both paths reach the supervisor for the same press.
33. **Staged reports.** The supervisor reports on each stop request in stages,
    requested, cancellation acknowledged, worker processes stopped, resources
    reusable and the final state, and sends the reports back to the requester
    through the control endpoint. The display server forwards them to the shell,
    which shows them. The stop-request port reports only accepted and rejected
    so far; the stages will be added as a compatible revision.
34. **Registering the foreground session.** The display server can register the
    current foreground session with the supervisor, so that the emergency path
    of a long press, which does not go through the display server, finds the
    session to stop. The focus core keeps the foreground session, as [epoch and
    focus generation](../architecture.md#epoch-and-focus-generation) states, and
    the stop-request port will get the registration as a method with a default
    implementation, added as a compatible revision.

## Memory pressure

35. **A memory pressure signal.** krepi signals memory pressure to GUI
    processes, the display server and applications. kosmo then drops first the
    GUI caches that can be rebuilt; GUI caches are accounted apart from
    computation memory. The form of the signal and its notification protocol are
    krepi's decisions.

## Restricted modes

36. **Policy summary code.** krepi gives a short policy summary code for a
    restricted mode and lets the display server query the current restricted
    state and its code and learn when they change. The display server shows them
    in the trusted indicator area, where they can be checked. It draws variable
    content with the fixed glyph set of [trusted
    presentation](../architecture.md#trusted-presentation), which has digits and
    hexadecimal digits, so the code is written with those characters. While the
    state cannot be obtained, the indicator never shows normal.
37. **The clipboard on entering and leaving.** kosmo needs krepi's specification
    of restricted modes to state that the system clipboard is cleared both when
    a restricted mode is entered and when it is left. kosmo's clipboard
    mediation in the display server clears it when the server learns of the
    change (requirement 36).
38. **Notifications during a restricted mode.** kosmo needs the specification to
    state that application notifications are suppressed during a restricted
    mode. kosmo's shell suppresses them.
39. **Screenshots during a restricted mode.** kosmo needs the specification to
    state that during a restricted mode a screenshot needs the permission of the
    policy and carries a mark in the image. kosmo's display server authorizes
    screenshots and adds the mark.

## The display server as a supervised service

40. **The epoch from the supervisor.** When krepi's supervisor starts the
    display server, it gives the epoch of this service instance through krepi's
    start and recovery interface: at least 1, different for different instances
    and larger for an instance started later. The server never produces its
    epoch, as [epoch and focus
    generation](../architecture.md#epoch-and-focus-generation) states.
41. **Restart.** When the display server ends, by exiting or by crashing, the
    supervisor restarts it as a new instance, with a new epoch (requirement 40).
42. **Device services for the new instance.** krepi's device rules state that a
    holder that dies loses only its mappings, the scan-out goes on and the last
    frame stays on the display. kosmo needs the display and input device
    services to be given to the new instance of the display server when it
    restarts, the input device services still to one holder only, and the grants
    of the old instance to become invalid. The new instance then reads the
    output enumeration, allocates its scan-out buffers and composes each output
    whole, as after an epoch change in the [holder
    handover](../architecture.md#holder-handover).
43. **Clients reconnecting.** krepi's [IPC
    rules](https://github.com/casxi-lab/krepi/blob/e099b40/docs/domains/ipc-waits/ipc/contract.md#release-and-peer-closure)
    end a call with the outcome peer closed when the process of the server dies.
    kosmo needs a client to learn that the old instance of the display server
    has ended and to find the new instance, with its new epoch and new endpoint,
    so that it can set up its session again; how the client finds it is krepi's
    decision. The client library drops events of another epoch, and reconnection
    is added later, as [epoch and focus
    generation](../architecture.md#epoch-and-focus-generation) states.

## Traces from input to presentation

44. **Trace numbers from the driver.** The records of an input device service
    carry a trace number, or an identifier that can be correlated, from the
    driver's acknowledgement on, so that the chain from input to presentation
    can match the points of the driver, the display server, the framework and
    the hardware-in-the-loop measurement by trace number. kosmo will append
    trace numbers to its key events and commits as a compatible revision, as
    [messages](../architecture.md#messages) states.
45. **The display server's trace points in krepi's evidence.** krepi's evidence
    of the latency from input to presentation on the device, its
    hardware-in-the-loop receipts and traces, includes the display server's
    three trace points: taking an event from the input device service, handing
    the event to a client, and handing the composed result to the device, and
    splits the whole latency into kosmo's segments by trace number. kosmo
    defines these trace points in the display server.

## Pointing devices

46. **USB pointing devices.** krepi's device rules state that the input drivers
    cover USB HID keyboards and keyboards on RP1 I2C. kosmo needs them also to
    support USB pointing devices and to give normalized pointer events through
    an input device service, for Linux programs on the compatibility path.
    Pointer records will be added to kosmo's [device
    record](../architecture.md#device-record) by a design change, as [time,
    decoding and scripts](../architecture.md#time-decoding-and-scripts) states.

## Linux compatibility

kosmo's compatibility service, which will live in a separate repository, shows
unmodified Linux desktop programs on the display server. It runs on krepi's
Linux compatibility.

47. **Unix sockets.** krepi's Linux compatibility gives Linux programs `AF_UNIX`
    sockets, through which Wayland and X11 clients connect to the compatibility
    service.
48. **Passing descriptors.** krepi's Linux compatibility passes file descriptors
    over `AF_UNIX` sockets with `SCM_RIGHTS`, which the compatibility layer maps
    to transfers of capabilities.
49. **Anonymous shared memory files.** krepi's Linux compatibility provides
    memfd, anonymous files of shared memory, in which clients hand their buffers
    to the compatibility service.
50. **File seals.** krepi's Linux compatibility provides seals on memfd files,
    such as the seal that forbids shrinking, so that a client cannot change the
    size of shared memory while the compatibility service reads it. kosmo's
    [host transport](../architecture.md#host-transport) requires the same seal
    of a buffer on a Linux host.
51. **Display paths in the namespace.** The authorized namespace of a Linux
    program provides the paths that display needs: `$XDG_RUNTIME_DIR`, the
    directory of the X11 sockets, and the paths of fonts and fontconfig.
52. **Both sides in one process.** The process of the compatibility service can
    both be a client of kosmo's display protocol through krepi's native IPC and
    serve the sockets of Linux programs in the Linux compatibility layer.
53. **Linux user-space components.** The Linux user-space components that the
    compatibility environment needs, such as Xwayland, the D-Bus daemon,
    fontconfig and a software GL implementation, are part of the Linux user
    space of fixed versions that krepi's Linux compatibility provides: krepi
    packages them and reviews their licenses. kosmo delivers only the
    compatibility service and the portals.

## Building kosmo for the device

54. **Aligned dependencies.** When krepi's integration builds kosmo's crates,
    the external packages that it uses have the versions in the `Cargo.lock` of
    the matching kosmo workspace at the tag that it pins. kalyx is one of them,
    so the image never holds two copies of kalyx. kosmo has three product
    workspaces, each with its own `Cargo.lock`, as the [source
    layout](../architecture.md#source-layout) states.

## Client transport

On the device, krepi's integration implements the [client transport
port](../architecture.md#client-transport-port) on krepi's IPC and shared
memory.

55. **Sessions and the peer's identity.** krepi's IPC gives the receiver of a
    call the
    [badge](https://github.com/casxi-lab/krepi/blob/e099b40/docs/domains/ipc-waits/ipc/contract.md#badges-and-sender-identity)
    of the capability through which the caller called, which the party that
    minted that capability chose, and delivers no other identity of the caller;
    its [notifications and wait
    sets](https://github.com/casxi-lab/krepi/blob/e099b40/docs/architecture.md#ipc-notifications-and-waits)
    let one wait cover every session and every device source. kosmo needs a
    client to set up a session with the display server through an endpoint that
    krepi grants it, with a badge that the granting party mints to identify the
    client, so that the badge that the kernel delivers with each call is the
    peer's unforgeable identity. How a client obtains the endpoint, for example
    through a directory of services, is krepi's decision. The server looks the
    identity up in its grant table (requirement 63).
56. **The frames of a call and its reply.** krepi's
    [IPC](https://github.com/casxi-lab/krepi/blob/e099b40/docs/domains/ipc-waits/ipc/contract.md#terms)
    carries two words of 64 bits in a call and two in its reply, and its [IPC
    rules](https://github.com/casxi-lab/krepi/blob/e099b40/docs/architecture.md#ipc-notifications-and-waits)
    send large data through pre-authorized shared buffers. kosmo needs each call
    to carry one frame and its reply one frame, each at most 512 bytes, a bound
    that grows only with a new version of the protocol, and each handed over
    whole: the frame of a call can be read in full by the server when it
    receives the call, and the frame of a reply by the client when it receives
    the reply. Where the frames lie, for example in slots of memory that the
    session shares, is krepi's decision. Messages of protocol v0 are up to 292
    bytes long, as the [wire format](../architecture.md#wire-format) states.
57. **The request queue.** Each session's request queue lies in memory that the
    client and the server share, and the server can read its write position. The
    frames that the client puts into the request queue before a call are visible
    to the server when it receives the call. A call carries the count of these
    frames, a 64-bit number that fits in krepi's two-word message, and the
    server processes the queue up to that count before the call, as the
    [transport rules](../architecture.md#transport-rules) state.
58. **The read position of the event queue.** The server can read the position
    up to which the client has taken events from the session's event queue. When
    the client takes an event and then puts a request or makes a call, the
    server sees that taking when it reads the request or receives the call. The
    server uses the read position to count the free slots of the event queue and
    to end an overflow.
59. **The capacity of the event queue.** Each session's event queue holds
    `C_E + R` frames, where `R` is the part kept for events that must arrive,
    with a capacity that the server gives when the session is set up. krepi
    never drops or overwrites a frame that has been put and not yet taken, and
    putting a frame never blocks the server. The server itself counts the slots
    that ordinary events and events that must arrive use, and which kind of
    queue carries them is krepi's decision. In the first release `C_E` is 64 and
    `R` is the session's quota of buffers plus 2, as the [transport
    rules](../architecture.md#transport-rules) state.
60. **One memory object with a call.** krepi's IPC carries at most one
    capability with a call, and the message and the capability become visible to
    the receiver together, as its [capability
    transfer](https://github.com/casxi-lab/krepi/blob/e099b40/docs/domains/ipc-waits/ipc/contract.md#capability-transfer)
    states; its memory rules provide no shared objects at first. kosmo's call
    that creates a buffer carries exactly one capability, to a shared memory
    object, which is the object of requirement 20; kosmo needs krepi's IPC to
    transfer a capability to such an object with a call. The one-shot [reply
    capability](https://github.com/casxi-lab/krepi/blob/e099b40/docs/domains/ipc-waits/ipc/contract.md#replies)
    of each call already gives the one reply slot of a session that the
    transport rules need; the frame of the reply is requirement 56.
61. **A peer that has gone.** The display server learns that the peer of a
    session has gone, because the client's process ended or the client closed
    the session, so that it can end the session and reclaim its buffers and
    objects. krepi's [death
    notifications](https://github.com/casxi-lab/krepi/blob/e099b40/docs/domains/objects-capabilities/lifecycle/contract.md#death-notifications)
    report the end of a process, but they need the right to observe each
    client's process, leave observers other than the owner three registration
    slots per process, and say nothing when a client closes a session while its
    process lives on. The outcome peer closed tells only a caller that the
    server has gone, and the server never calls its clients.

## Settings, roles and acceptance

62. **A system configuration area.** krepi's file service provides a system
    configuration area in which kosmo's display server keeps system settings
    persistently, such as the input settings: repeat, long press, key maps and
    layer state. kosmo is the authority for these settings, as the [key
    maps](../architecture.md#key-maps) state, and krepi stores them.
63. **The source of role grants.** On the device, the grant table that the
    display server looks up when it grants roles to a session, application or
    shell, comes from krepi's application registration and is looked up by the
    identity that the session's authenticated endpoint gives (requirement 55),
    as [sessions, roles and
    placement](../architecture.md#sessions-roles-and-placement) states.
64. **kosmo's interfaces in krepi's acceptance.** When krepi accepts interactive
    widgets, experiment data capture, peripheral control and experiment
    programming, exchange with a computer companion, and learning activities
    with questions, the acceptance takes kosmo's user interface for each of them
    as an input.

## Form of joint acceptance input

Joint tests that connect kosmo to krepi's services or to the device belong to
krepi, as [tests](../../CONTRIBUTING.md#tests) states. kosmo gives krepi their
input in this form:

| Input | Form |
| --- | --- |
| Event scripts | Records and lines as [time, decoding and scripts](../architecture.md#time-decoding-and-scripts) and [headless backend and scripts](../architecture.md#headless-backend-and-scripts) define them. The [server-side script runner](../architecture.md#server-side-script-runner) states how the key lines are projected and injected and how time and checkpoint lines are handled on the server's side. The key lines of a script given to krepi are presses and releases only; projected to device records, they form the sequence of presses and releases that is injected on the device. Repeat and long press come from how long a key is held between its press and its release, and the display server produces them. The text format of the header, the time lines and the checkpoint lines is fixed with the script runner, as those sections state |
| Expected records | Later |
| Region digests | Later |
