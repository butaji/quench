const common = require('../common');
if (!common.canCreateSymLink()) throw new Error('symlink capability unexpectedly disabled');
