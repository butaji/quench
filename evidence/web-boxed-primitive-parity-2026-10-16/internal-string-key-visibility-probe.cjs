const key='\0quench:user-property',object={[key]:1};
console.log(JSON.stringify([Reflect.ownKeys(object),Object.keys(object),Object.getOwnPropertyNames(object),JSON.stringify(object)]));
