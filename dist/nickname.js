(() => {
 const adjectives=[['Новогодний','Новогодняя'],['Белый','Белая'],['Снежный','Снежная'],['Зимний','Зимняя'],['Лунный','Лунная'],['Северный','Северная'],['Сонный','Сонная'],['Весёлый','Весёлая'],['Звёздный','Звёздная']];
 const nouns=[['эльф',0],['снежинка',1],['олень',0],['лиса',1],['снеговик',0],['звезда',1],['барс',0],['метель',1],['пингвин',0],['искра',1]];
 const field=document.getElementById('nickname');
 document.getElementById('random-nickname').onclick=()=>{let name;do{const [noun,gender]=nouns[Math.floor(Math.random()*nouns.length)];name=adjectives[Math.floor(Math.random()*adjectives.length)][gender]+' '+noun}while(name===field.value||name.length>24);field.value=name;field.dispatchEvent(new Event('input',{bubbles:true}));field.focus()};
})();
