function check(value, label) {
    if (!value) throw new Error(label);
}
function releaseSparseArray() {
    var garbage = [];
    for (var i = 0; i < 2048; i++) garbage.push({index: i});
    var sparse = [];
    sparse[1000000] = 37;
    check(sparse.length === 1000001, 'seed length');
    check(sparse[1000000] === 37, 'seed value');
}

releaseSparseArray();
$262.gc();
var reusedArray = [];
check(reusedArray.length === 0, 'reused array length');
check(reusedArray[1000000] === undefined, 'reused array indexed read');
check(!(1000000 in reusedArray), 'reused array inherited/indexed presence');
check(!reusedArray.hasOwnProperty(1000000), 'reused array own presence');
reusedArray[3] = 29;
check(reusedArray.length === 4 && reusedArray[3] === 29, 'reused array ordinary write');
check(!reusedArray.hasOwnProperty(1000000), 'reused array high index remains absent');

releaseSparseArray();
$262.gc();
var reusedObject = {visible: 41};
check(reusedObject.visible === 41, 'reused object own data');
check(!reusedObject.hasOwnProperty(1000000), 'reused object has no stale indexed property');
check(!('length' in reusedObject), 'reused object has no stale array length');
