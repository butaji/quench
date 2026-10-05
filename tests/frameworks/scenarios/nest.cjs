require('reflect-metadata'); const { Readable } = require('node:stream'); const path = require('node:path');
const { NestFactory } = require('@nestjs/core'); const c = require('@nestjs/common');
class AppController { root() { return '<p>hi</p>'; } echo(body) { return body; } stream() { return new c.StreamableFile(Readable.from(['a', 'b', 'c'])); } asset(res) { return res.sendFile(path.join(__dirname, 'asset.txt')); } }
c.Controller()(AppController);
const def = (m, d) => Reflect.decorate(d, AppController.prototype, m, Object.getOwnPropertyDescriptor(AppController.prototype, m));
def('root', [c.Get('/')]); def('echo', [c.Post('/echo')]); def('stream', [c.Get('/stream')]); def('asset', [c.Get('/asset.txt')]);
c.Body()(AppController.prototype, 'echo', 0); c.Res()(AppController.prototype, 'asset', 0);
class AppModule {} c.Module({ controllers: [AppController] })(AppModule);
exports.start = async () => { const a = await NestFactory.create(AppModule, { logger: false }); await a.listen(0); const port = a.getHttpServer().address().port; return { port, stop: () => a.close() }; };
