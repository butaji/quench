const __quenchDgramMembership = (socket, operation, address) => {
  if (address === undefined) {
    throw Object.assign(
      new TypeError('The "multicastAddress" argument must be specified'),
      {
        code: "ERR_MISSING_ARGS",
      },
    );
  }
  if (address !== "224.0.0.114") throw new Error(`${operation} EINVAL`);
  if (!socket._bound) {
    throw Object.assign(new Error("Not running"), {
      code: "ERR_SOCKET_DGRAM_NOT_RUNNING",
    });
  }
};
const __quenchDgramMembershipMethods = (socket) => ({
  addMembership: (address) =>
    __quenchDgramMembership(socket, "addMembership", address),
  dropMembership: (address) =>
    __quenchDgramMembership(socket, "dropMembership", address),
  addSourceSpecificMembership: (source, group) =>
    __quenchSourceMembership(
      socket,
      "addSourceSpecificMembership",
      source,
      group,
    ),
  dropSourceSpecificMembership: (source, group) =>
    __quenchSourceMembership(
      socket,
      "dropSourceSpecificMembership",
      source,
      group,
    ),
});
const __quenchSourceMembership = (socket, operation, source, group) => {
  if (typeof source !== "string") {
    throw Object.assign(
      new TypeError(
        'The "sourceAddress" argument must be of type string. Received type number (0)',
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (typeof group !== "string") {
    throw Object.assign(
      new TypeError(
        'The "groupAddress" argument must be of type string. Received type number (0)',
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (source === "0" || group === "0") {
    throw Object.assign(new Error(`${operation} EINVAL`), {
      code: "EINVAL",
    });
  }
  if (!socket._bound) {
    throw Object.assign(new Error("Not running"), {
      code: "ERR_SOCKET_DGRAM_NOT_RUNNING",
    });
  }
};
