// jetstream-row: raytrace-private-class-fields
// jetstream-files: class-fields/raytrace-private-class-fields.js
// iterations: 120
/* Same algorithm as Octane raytrace (raytrace.c): the private class-fields variant differs only in
 * class-field syntax (#checkNumber, #options, #testIntersection); it runs the same 15 renderScene()
 * calls per iteration with the same checkNumber === 2321 validation, so raytrace.c is reused. */
#include "raytrace.c"
