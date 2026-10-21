// build-house.rhai — drive a document entirely from script.
//
// Run with:  freecad-rs script build-house.rhai
//
// Builds a small house model: a box base slab, a cylinder chimney whose
// height is bound parametrically to the base length (FreeCAD
// setExpression), and a sphere lifted by a Part::Transform — then
// recomputes, reads back the exact analytic stats, saves the document and
// exports a mesh. The transcript is what `freecad-rs script` prints.

let doc = new_document("House");

// The base slab: Part::Box, 30 x 20 x 5 mm.
let base = doc.add_box("Base", 30.0, 20.0, 5.0);

// A chimney on top, driven parametrically: Height = 2 * Base.Length -> 60.
// The expression engine owns the semantics — dependency edges, cycle
// rejection, honest type checks — exactly like the CLI's set-expr.
let chimney = doc.add_cylinder("Chimney", 1.5, 12.0);
doc.bind(chimney, "Height", "2 * Base.Length");

// A ball, lifted 14 mm by a Part::Transform. The transform's Placement
// moves geometry; a primitive's own Placement property is carried but not
// baked into its analytic shape (kernel convention).
let ball = doc.add_sphere("Ball", 4.0);
let lift = doc.add_transform("Lift", ball);
doc.set(lift, "Placement", placement(vec3(0.0, 0.0, 14.0), vec3(0.0, 0.0, 1.0), 0.0));

// Recompute dependencies-first; the transcript records what ran.
doc.recompute();

// Read back the exact analytic stats (same numbers the `stats` verb
// prints): the box is exact — 30 * 20 * 5.
print("Base volume: " + doc.volume(base));
print("Base area:   " + doc.area(base));
print("Chimney volume (Height = 2 * Base.Length): " + doc.volume(chimney));

// Persistence is the script's choice: explicit save + mesh export.
doc.save("house.FCStd");
doc.export_stl("house.stl", false);
