// TypeScript enum numeric value
// TypeScript type erasure - enums become objects

let Color;
(function (Color) {
    Color[Color["Red"] = 1] = "Red";
    Color[Color["Green"] = 2] = "Green";
})(Color || (Color = {}));
Color.Red;
