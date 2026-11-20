// jetstream-row: raytrace-public-class-fields
// jetstream-files: class-fields/raytrace-public-class-fields.js
// iterations: 120
/* Same algorithm as Octane raytrace (raytrace.c): the public class-fields variant differs only in
 * class-field syntax; it runs the same 15 renderScene() calls per iteration with the same
 * checkNumber === 2321 validation, so the Octane C analogue is compiled unchanged. */
#include "raytrace.c"
