# Requirements on typed values

This document lists what kosmo saves or exchanges and what it needs from the
typed values that hold it: the editing structure of a formula, the three
versions, exact results, errors with their places, the entries of the
Scratchpad history and the Scratchpad's separation from documents, and three
rules for every typed value that kosmo keeps: the type tags and schema
versions of source content, display text that is only a cache, and the
validity of results. It ends with the kinds of content that kosmo will save or
exchange later. It is written for people who build kosmo or a component that
provides typed values to it, such as the document service that stores
documents and their values or the computer algebra system that computes
results.

The numbered requirements are kosmo's needs. Each states what kosmo saves or
exchanges and what typed values must provide for it: the representation, the
version marks, the size bound and the handling of unknown types, and says so
where one of these does not apply. How a provider provides it, including the
encoding and the byte layout, is the provider's choice, and a provider can
accept or reject each requirement on its own.

kosmo saves nothing yet. All of its state is in memory, and it exchanges with
kalyx, the computer algebra system that it uses, only the linear text of a
formula, as [formula conversion](../features/math/formula-conversion.md)
describes, and the output lines that hold the results. So each requirement also
states what holds now and what it leaves to the provider, and names the types
and tests of kosmo that implement or check the current state: types with their
crate, unit tests with their module, such as `version::tests::…`, and
integration tests with their file in the `tests/` directory of the crate, such
as `tests/history.rs`. The requests, reports and terminal states of jobs belong
to the [job layer](../features/framework/job-layer.md) and are not listed here.

A requirement that a provider rejects does not change kosmo's design in this
document; kosmo's design changes only through its architecture, whose
[editing structure](../architecture.md#editing-structure) and
[versions](../architecture.md#versions) sections state the rules that these
requirements rest on.

## Content of the first release

1. **The editing structure.** The source content of a formula, which kosmo
   will save in documents and which it converts into linear text for kalyx.
   - Representation: an ordered map keyed by block identity, without
     pointers. A block is a sequence of items; an item is a character or a
     template, and a template carries its identity and its kind. Identities
     are 64-bit unsigned integers that one editing buffer allocates in
     increasing order and never reuses; the root block takes the first, and
     the same edits run twice give the same identities. Holes inside a block
     are not stored: parsing derives them. The cursor and the selection are
     not source content. A snapshot is an immutable copy of the structure
     with its source version, as
     [node identity](../architecture.md#node-identity) states.
   - Version marks: the source version of requirement 2. Snapshots and
     history entries carry the source version at which they were taken.
   - Size bound: a line holds at most 4,096 items from a character set of 72
     ASCII characters, each of which is one item and one byte; one insertion
     holds at most 4,096 bytes; the parse depth is at most 64. The first
     release has no templates, so the block nesting depth is always 1. The
     [limits of one-line editing](../features/math/one-line-editing.md#limits)
     list them.
   - Unknown types: the kinds of items and of templates only grow and never
     change; their enums are `#[non_exhaustive]`. Conversion writes an item
     that is not a character as U+FFFD, which kalyx rejects; the first
     release cannot build such an item, so no test covers it. Saved content
     of a kind that kosmo does not know is handled as requirement 7 states.
   - Now: the structure is in memory only and is not serialized. For kalyx, a
     snapshot is converted into linear text, as
     [linear text](../features/math/formula-conversion.md#linear-text)
     describes.
   - Left to the provider: the format and the byte layout in which the
     structure is saved.
   - Types and tests: in `kosmo-math-model`, `EditBuffer`, `Snapshot`,
     `Item`, `NodeId`, `MAX_ITEMS`, `MAX_INSERT_BYTES` and
     `MAX_PARSE_DEPTH`; in `kosmo-math-kalyx`, `convert`. Tests in
     `kosmo-math-model`: `buffer::tests::the_character_set_has_72_characters`,
     `tests/limits.rs`: `inserting_into_a_full_line_is_rejected`,
     `tests/versions.rs`:
     `the_same_edits_twice_give_the_same_structure_identities_and_versions`,
     `tests/results.rs`:
     `a_snapshot_keeps_its_content_while_the_buffer_changes`.
2. **The three versions.** The versions that every request, result and
   history entry carries.
   - Representation: three distinct 64-bit unsigned counters. The source
     version numbers the content of an editing buffer, the context version
     the context of a region, which [Scope](../scope.md) calls the
     environment version, and the settings version the semantic settings of
     a settings holder. Each is compared only within the buffer, region or
     holder that numbers it, and the three travel together with every
     request, result and history entry, as
     [three version types](../architecture.md#three-version-types) states.
   - Version marks: the versions are the marks themselves. A rendering of a
     result writes only the source and settings versions; whatever saves or
     exchanges a result carries all three.
   - Size bound: each starts at 0, only increases and never wraps. A change
     that would increase one past `u64::MAX` is rejected and leaves the
     structure, the cursor and the versions unchanged. A submitted source
     version is at least 1.
   - Unknown types: the triple has exactly three fields and is not
     `#[non_exhaustive]`, so a fourth version is an incompatible change. When
     the versions echoed with a result differ from those of its request, the
     job fails or its output line is unreadable.
   - Now: the context version is always 0, because no operation changes the
     context, and the settings version is 0 outside tests.
   - Types and tests: in `kosmo-math-model`, `SourceVersion`,
     `ContextVersion`, `SettingsVersion` and `Versions`. Tests in
     `kosmo-math-model`: `version::tests::starts_at_zero_and_steps_by_one`,
     `version::tests::has_no_version_after_the_maximum`,
     `buffer::tests::an_edit_past_the_maximum_version_is_rejected`,
     `display::tests::the_context_version_is_not_written`; in
     `kosmo-math-kalyx`: `output::tests::other_echoed_versions_are_unreadable`.
3. **Exact results.** A value or a mathematical state that a job computes.
   - Representation: a value is represented by its display text in an output
     line of kalyx, non-empty printable ASCII text that kalyx reads back as
     the same value, and a state by its name. Display text is the only output
     format of the locked kalyx version, and its bytes are relied on for that
     version only, so this representation is transitional: it will be
     replaced by a versioned encoding of values from the computer algebra
     system, with a type tag, an encoding version and bytes. Requirement 8
     states what display text is once that encoding exists.
   - Version marks: the three versions of requirement 2. A saved result must
     also tell which version of the computer algebra system computed it,
     because display bytes hold only for the locked version.
   - Size bound: display text holds at most 65,536 bytes when it is shown; a
     rendering cuts longer text and states its length. An output line is
     bounded by the output limit of its request, 65,536 bytes by default. The
     text of a value that can be read back for reuse holds at most 4,096
     bytes.
   - Unknown types: a state whose name kosmo does not know is shown by its
     name. An output line that cannot be read becomes unreadable content,
     which is shown as an internal error; reading a line never panics. The
     kinds of result content only grow; their enum is `#[non_exhaustive]`.
   - Now: a value exists only as its display text, which is read back into an
     editing structure when the value is reused, as
     [reading back](../features/math/formula-conversion.md#reading-back)
     describes.
   - Left to the provider: the encoding of values, which the computer algebra
     system defines, and how a result carries the version of the computer
     algebra system that computed it.
   - Types and tests: in `kosmo-math-model`, `DisplayItem`, `Content` and
     `MAX_DISPLAY_BYTES`; in `kosmo-math-kalyx`, `read_output_line`. Tests in
     `kosmo-math-model`: `display::tests::each_content_has_its_head`; in
     `kosmo-math-kalyx`: `tests/displayed_values.rs`:
     `the_value_line_of_each_answer_renders_as_expected`,
     `tests/bounded_display.rs`:
     `the_bound_is_the_default_output_limit_of_kalyx`,
     `output::tests::the_other_four_states_and_a_later_one_render_by_name`,
     `output::tests::an_unknown_kind_of_line_is_unreadable`,
     `tests/read_back.rs`: `answers_longer_than_one_insertion_are_not_reusable`.
4. **Errors with their places.** An error that a job reports, placed in the
   input that was submitted.
   - Representation: an error is its kind and its place in the snapshot that
     was submitted. The kind is one of the ten kinds of the locked kalyx
     version or a later kind kept by its name; a resource limit also carries
     the name of the limit, its scope, the allowed value and the amount. The
     place is the whole line, a fragment, which is a block and a half-open
     item range, or one position between items. kalyx gives a half-open range
     of bytes of the linear text, which kosmo maps back to the editing
     structure, as
     [error ranges](../features/math/formula-conversion.md#error-ranges)
     describes. The wording of the message is not part of the interface and
     is not read.
   - Version marks: the three versions of its result. The place is valid only
     in the source version of the submitted snapshot and is never marked on
     input of another source version.
   - Size bound: the range lies within the linear text, which holds at most
     5,461 bytes for one line; the item index of a position is at most 4,096.
   - Unknown types: a later kind is kept by its name, and its limit fields are
     not read. A range that runs backwards or past the end of the text, and a
     line whose fields are malformed, give unreadable content.
   - Now: errors are in memory only.
   - Types and tests: in `kosmo-math-model`, `ErrorKind`, `Place` and
     `LimitExceeded`; in `kosmo-math-kalyx`, `map_span`. Tests in
     `kosmo-math-kalyx`: `tests/error_spans.rs`:
     `an_error_without_a_span_maps_to_the_whole_line`,
     `span::tests::a_span_outside_the_text_is_rejected`,
     `tests/displayed_errors.rs`:
     `a_stale_error_keeps_its_place_in_the_submitted_line`,
     `output::tests::a_later_kind_is_kept_by_name_without_reading_its_limit_fields`.
5. **Scratchpad history entries.** The record of each submission of the
   Scratchpad with its result.
   - Representation: an entry is one accepted submission: the identity of its
     job, a 64-bit number that is never reused; the snapshot of the input as
     it was submitted (requirement 1); its three versions (requirement 2);
     its operation, evaluation, expansion or differentiation; its variable;
     and its result, written at most once: the terminal state, the stop
     reason, whether the job failed with an internal error, and the display
     item (requirements 3 and 4). An entry is a copy of its own: editing the
     current line, reusing the entry and later results change none of it, as
     the [history](../features/framework/scratchpad.md#history) describes.
   - Version marks: the three versions at which it was submitted. A late
     result goes only into the entry of its own versions.
   - Size bound: the history holds at most 32 entries and evicts the oldest
     first when it is full; a variable holds at most 4,096 bytes. There is no
     separate bound on bytes: an input holds at most 4,096 items, and the
     output line of a result at most the larger of 65,536 bytes and the
     output limit of its job.
   - Unknown types: the kinds of operations and of terminal states only grow.
     Reuse refuses result content of a kind that it does not know and changes
     nothing; the first release cannot build such content, so no test covers
     it.
   - Now: entries are in memory only.
   - Types and tests: in `kosmo-scratchpad`, `Entry`, `Outcome`,
     `MAX_HISTORY_ENTRIES` and `MAX_VARIABLE_BYTES`; in `kosmo-jobs`,
     `Operation`. Tests in `kosmo-scratchpad`: `tests/history.rs`:
     `the_oldest_entry_is_evicted_past_the_bound` and
     `a_late_result_goes_only_into_the_entry_of_its_own_version`,
     `tests/reuse.rs`: `editing_after_reuse_changes_no_entry`,
     `scratchpad::tests::a_second_delivery_of_a_terminal_state_leaves_the_entry_as_it_was`.
6. **The Scratchpad's separation from documents.** What keeps the
   Scratchpad's work out of documents.
   - Representation: all of the Scratchpad's state is in memory. It reads and
     writes no file and refers to no document, and its entries hold no path,
     file or document. Dropping the Scratchpad loses its work. Its work
     enters a document only when you ask for it to be copied, as the copying
     from the Scratchpad under [later content](#later-content) describes;
     until then nothing of it reaches a document, as
     [memory only](../features/framework/scratchpad.md#memory-only) states.
   - Version marks: not applicable, because the separation is a property and
     carries no value.
   - Size bound: those of requirement 5.
   - Unknown types: not applicable, for the same reason.
   - Now: the Scratchpad's work cannot be saved as a document.
   - Types and tests: in `kosmo-scratchpad`, `Scratchpad`, and the test
     `tests/memory_only.rs`: `the_scratchpad_writes_no_file`.

## Every typed value

7. **Type tags and versions of source content.** How kosmo's source content
   is saved in a document.
   - Representation: every kind of source content that kosmo defines, in the
     first release only the editing structure of one line, goes to the
     document service with a type tag, kosmo's own schema version and a
     bounded number of bytes. The document service stores the bytes and does
     not interpret them. The same edits give the same content.
   - Version marks: the schema version has a major and a minor part. The
     major version increases only when the meaning of something that exists
     changes; additions are optional fields of a new minor version. The
     content also carries its source version.
   - Size bound: the bytes are bounded, and a reader compares the length with
     the bound before it allocates. The interface of the document service
     fixes the number, which is at least enough for one line of kosmo content
     within kosmo's own bounds, 4,096 items in one block; kosmo then checks
     the bounds of requirement 1.
   - Unknown types: content of a major version that kosmo does not know is
     not edited; kosmo shows a placeholder and saves the content as it was.
     Optional fields of a minor version that kosmo does not know are skipped
     and saved as they were.
   - Now: there is no type tag, schema version or serialization; kinds of
     content differ only by the variants of `Item`, and the content carries
     its source version.
   - Left to the provider: how type tags and schema versions are written, and
     the number of the byte bound.
   - Types and tests: in `kosmo-math-model`, `Item`, `TemplateKind` and
     `SourceVersion`, and the test `tests/versions.rs`:
     `the_same_edits_twice_give_the_same_structure_identities_and_versions`.
8. **Display text is only a cache.** How kosmo keeps the results that it
   shows.
   - Representation: the authoritative representation of a result is its
     typed value. Display text is rebuilt from the value and the display
     settings; it is not a required field, and it can be dropped when memory
     runs low or after a restart and built again. Copying and reuse take the
     typed value, not the display text.
   - Version marks: display text changes with the settings version. Options
     that only change the presentation do not enter the settings version, and
     changing them only lays the result out again.
   - Size bound: a rendering shows at most 65,536 bytes of text; longer text
     is cut and states its length.
   - Unknown types: a value of a kind that kosmo does not know is kept: kosmo
     shows a placeholder, never drops the value and never reuses it as text.
   - Now: transitional, as requirement 3 states: a value exists only as its
     display text, which is read back when the value is reused.
   - Types and tests: in `kosmo-math-model`, `DisplayItem`; in
     `kosmo-math-kalyx`, `read_back`. Tests in `kosmo-math-model`:
     `display::tests::rendering_twice_gives_the_same_text`,
     `display::tests::longer_text_is_cut_and_states_its_length`; in
     `kosmo-math-kalyx`: `tests/read_back.rs`:
     `answers_longer_than_one_insertion_are_not_reusable`.
9. **Validity of results by invalidation marks or dependency versions.** How
   kosmo tells whether a result still holds.
   - Representation: kosmo can decide whether a result that it saved or
     received is still valid, in either of two ways: the document service
     marks the results that each commit invalidates, or each result carries
     the versions of the content it depends on, which kosmo compares. The
     decision never depends on display text.
   - Version marks: the three versions and the submission that the result
     belongs to, and, once a provider gives them, the invalidation marks or
     the dependency versions.
   - Size bound: the set of invalidated results and the dependency versions of
     a result are bounded; the interface of the provider fixes the numbers.
   - Unknown types: a result whose validity cannot be decided is handled as
     stale: it is not written into the current result slot and is shown as
     stale.
   - Now: a result is judged only by whether its three versions equal the
     current ones and whether it belongs to the latest submission, as
     [current, superseded and stale results](../architecture.md#current-superseded-and-stale-results)
     states; the context version is always 0, and results have no
     dependencies.
   - Left to the provider: which of the two ways it provides.
   - Types and tests: in `kosmo-math-model`, `judge`, `Judgement` and
     `Freshness`. Tests in `kosmo-math-model`:
     `version::tests::a_result_with_any_other_version_is_stale_whatever_its_submission`;
     in `kosmo-math-kalyx`: `tests/stale_results.rs`:
     `a_result_is_stale_once_the_line_changed_again`.

## Later content

The first release does not save or exchange the following. Each is listed with
what kosmo will need of it; its numbered requirements are added when kosmo
saves or exchanges it.

- **Approximate values.** A typed value with its precision, marked as
  approximate. kosmo shows it apart from exact values and never reads it back
  for reuse as an exact value; the computer algebra system defines its
  encoding.
- **Quantities with units.** A number and its unit together, as one typed
  value. kosmo's display and editing need the number, the unit and the edition
  of the set of constants that was used; the component that provides the value
  defines it.
- **Vectors and matrices.** A typed value with its row count, its column count
  and its elements in row-major order. In the editing structure the row and
  column counts of a matrix template are part of the template's kind, and
  adding the template increases the schema version of source content.
- **Table columns.** Columns as typed values. kosmo needs only the fields that
  the table view shows and edits: the name of the column, the kind of its
  elements and the row count; the component that provides the table and the
  document service define the values and their recomputation.
- **Sample sequences.** Each sampling fixes the expression, the parameter
  environment, the viewport and the output size as one version; the number of
  points is bounded, and older samples can be discarded. kosmo needs only the
  fields that it shows; the component that provides the samples defines the
  sampling.
- **Complex numbers.** Typed values that the computer algebra system defines.
  The complex domain mode is a semantic setting and counts in the settings
  version; the display form of complex numbers is only a presentation option
  and does not.
- **Geometric objects.** kosmo needs only the fields that the geometry view
  shows and edits: the identity, the kind and the version of each object, with
  drags submitted as semantic edits. The application that provides the objects
  defines them.
- **Proof objects.** Check records are versioned values, marked out of date
  when their source or dependencies change. kosmo needs only the fields that a
  proof region shows: the state, the goals and the places of diagnostics; the
  application that provides the records defines them.
- **Document structure.** Stable identities, order and bounds for each level:
  document, problem, page and region; the kinds of regions and the type tags of
  source content, as requirement 7 states. The document service defines the
  structure, and kosmo defines the content of its regions.
- **Commit history.** Commits of semantic edits are grouped by gesture and can
  be inverted; an operation with side effects is a barrier; the history is
  bounded and not persistent. kosmo sends undo commands and groups gestures,
  and the document service keeps the history.
- **Copying from the Scratchpad.** Only when you ask, the Scratchpad's work is
  copied into a problem in one atomic commit, with a copy plan and a table that
  resolves variable conflicts. Until then the Scratchpad refers to no document,
  as requirement 6 states.
- **Settings hierarchy.** Semantic settings come in layers, for the document,
  the problem, the region and the math box, and the version of the merged
  settings takes part in the versions of a result; options that only change
  the presentation do not enter the settings version. The first release has
  one settings holder for each Scratchpad.
