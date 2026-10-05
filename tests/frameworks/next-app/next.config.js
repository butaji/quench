// The scenario packages live one directory up; pin the workspace root there.
module.exports = { turbopack: { root: require('node:path').join(__dirname, '..') } };
