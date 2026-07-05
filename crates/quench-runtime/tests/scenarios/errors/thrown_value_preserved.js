// Thrown value preservation in catch
// Task 250 - verify thrown values are preserved

(function() {
    try {
        throw "caught";
    } catch (e) {
        return e;
    }
})();
