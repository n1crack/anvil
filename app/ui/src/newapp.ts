import { $, addAppFolder, call, el, filterOf, filters, invoke, mode, select, setMode, snap, tilde } from "./app";
import { termOut } from "./console";
import { late } from "./late";
import { ansi, clean } from "./term";

/* The New application sheet: a template list grouped by language, name, location and
 * the template's options; Create turns it into the creation's progress and live log. */

// Simple Icons (CC0), drawn in currentColor on a 24×24 box.
const BRANDS: Record<string, string> = {
  laravel:
    "M23.642 5.43a.364.364 0 01.014.1v5.149c0 .135-.073.26-.189.326l-4.323 2.49v4.934a.378.378 0 01-.188.326L9.93 23.949a.316.316 0 01-.066.027c-.008.002-.016.008-.024.01a.348.348 0 01-.192 0c-.011-.002-.02-.008-.03-.012-.02-.008-.042-.014-.062-.025L.533 18.755a.376.376 0 01-.189-.326V2.974c0-.033.005-.066.014-.098.003-.012.01-.02.014-.032a.369.369 0 01.023-.058c.004-.013.015-.022.023-.033l.033-.045c.012-.01.025-.018.037-.027.014-.012.027-.024.041-.034H.53L5.043.05a.375.375 0 01.375 0L9.93 2.647h.002c.015.01.027.021.04.033l.038.027c.013.014.02.03.033.045.008.011.02.021.025.033.01.02.017.038.024.058.003.011.01.021.013.032.01.031.014.064.014.098v9.652l3.76-2.164V5.527c0-.033.004-.066.013-.098.003-.01.01-.02.013-.032a.487.487 0 01.024-.059c.007-.012.018-.02.025-.033.012-.015.021-.03.033-.043.012-.012.025-.02.037-.028.014-.01.026-.023.041-.032h.001l4.513-2.598a.375.375 0 01.375 0l4.513 2.598c.016.01.027.021.042.031.012.01.025.018.036.028.013.014.022.03.034.044.008.012.019.021.024.033.011.02.018.04.024.06.006.01.012.021.015.032zm-.74 5.032V6.179l-1.578.908-2.182 1.256v4.283zm-4.51 7.75v-4.287l-2.147 1.225-6.126 3.498v4.325zM1.093 3.624v14.588l8.273 4.761v-4.325l-4.322-2.445-.002-.003H5.04c-.014-.01-.025-.021-.04-.031-.011-.01-.024-.018-.035-.027l-.001-.002c-.013-.012-.021-.025-.031-.04-.01-.011-.021-.022-.028-.036h-.002c-.008-.014-.013-.031-.02-.047-.006-.016-.014-.027-.018-.043a.49.49 0 01-.008-.057c-.002-.014-.006-.027-.006-.041V5.789l-2.18-1.257zM5.23.81L1.47 2.974l3.76 2.164 3.758-2.164zm1.956 13.505l2.182-1.256V3.624l-1.58.91-2.182 1.255v9.435zm11.581-10.95l-3.76 2.163 3.76 2.163 3.759-2.164zm-.376 4.978L16.21 7.087 14.63 6.18v4.283l2.182 1.256 1.58.908zm-8.65 9.654l5.514-3.148 2.756-1.572-3.757-2.163-4.323 2.489-3.941 2.27z",
  statamic:
    "M19.78 21.639c1.754 0 2.398-.756 2.398-2.607v-3.62c0-1.722.837-2.704 1.641-3.17.242-.145.242-.483 0-.644-.836-.531-1.64-1.642-1.64-3.122v-3.54c0-1.996-.548-2.575-2.302-2.575H4.123c-1.754 0-2.301.58-2.301 2.575v3.556c0 1.48-.805 2.59-1.641 3.122a.377.377 0 0 0 0 .643c.804.451 1.64 1.433 1.64 3.17v3.605c0 1.85.645 2.607 2.399 2.607zm-7.82-3.299c-1.883 0-3.25-.563-4.522-1.673a.891.891 0 0 1-.29-.676.83.83 0 0 1 .193-.563l.403-.515c.193-.242.402-.354.643-.354.274 0 .531.112.805.29a5.331 5.331 0 0 0 2.993.884c.885 0 1.593-.37 1.593-1.126 0-1.963-6.533-.885-6.533-5.294 0-2.366 1.93-3.685 4.441-3.685 1.77 0 3.074.515 4.04 1.126.24.161.402.483.402.805 0 .193-.049.37-.161.53l-.29.435c-.21.29-.45.435-.756.435-.21 0-.435-.08-.676-.193a5.07 5.07 0 0 0-2.398-.564c-.95 0-1.513.515-1.513 1.046 0 2.012 6.534.918 6.534 5.198 0 2.414-1.947 3.894-4.908 3.894z",
  symfony:
    "M24 12c0 6.628-5.372 12-12 12S0 18.628 0 12 5.372 0 12 0s12 5.372 12 12zm-6.753-7.561c-1.22.042-2.283.715-3.075 1.644-.878 1.02-1.461 2.229-1.881 3.461-.753-.614-1.332-1.414-2.539-1.761-.966-.297-2.015-.105-2.813.514-.41.319-.71.757-.861 1.254-.36 1.176.381 2.225.719 2.6l.737.79c.15.154.519.56.339 1.138-.193.631-.951 1.037-1.732.799-.348-.106-.848-.366-.734-.73.045-.15.152-.263.21-.391.052-.11.077-.194.095-.242.141-.465-.053-1.07-.551-1.223-.465-.143-.939-.03-1.125.566-.209.68.117 1.913 1.86 2.449 2.04.628 3.765-.484 4.009-1.932.153-.907-.255-1.582-1.006-2.447l-.612-.677c-.371-.37-.497-1.002-.114-1.485.324-.409.785-.584 1.539-.379 1.103.3 1.594 1.063 2.412 1.68-.338 1.11-.56 2.223-.759 3.222l-.123.746c-.585 3.07-1.033 4.757-2.194 5.726-.234.166-.57.416-1.073.434-.266.005-.352-.176-.355-.257-.006-.184.15-.271.255-.353.154-.083.39-.224.372-.674-.016-.532-.456-.994-1.094-.973-.477.017-1.203.465-1.176 1.286.028.85.819 1.485 2.012 1.444.638-.021 2.062-.281 3.464-1.949 1.633-1.911 2.09-4.101 2.434-5.706l.383-2.116c.213.024.441.042.69.048 2.032.044 3.049-1.01 3.064-1.776.01-.464-.304-.921-.744-.91-.386.009-.718.278-.806.654-.094.428.646.813.068 1.189-.41.266-1.146.452-2.184.3l.188-1.042c.386-1.976.859-4.407 2.661-4.467.132-.007.612.006.623.323.003.105-.022.134-.147.375-.115.155-.174.345-.168.537.017.504.4.836.957.816.743-.023.955-.748.945-1.119-.032-.874-.952-1.424-2.17-1.386z",
  next: "M18.665 21.978C16.758 23.255 14.465 24 12 24 5.377 24 0 18.623 0 12S5.377 0 12 0s12 5.377 12 12c0 3.583-1.574 6.801-4.067 9.001L9.219 7.2H7.2v9.596h1.615V9.251l9.85 12.727Zm-3.332-8.533 1.6 2.061V7.2h-1.6v6.245Z",
  nuxt: "M13.4642 19.8295h8.9218c.2834 0 .5618-.0723.8072-.2098a1.5899 1.5899 0 0 0 .5908-.5732 1.5293 1.5293 0 0 0 .216-.783 1.529 1.529 0 0 0-.2167-.7828L17.7916 7.4142a1.5904 1.5904 0 0 0-.5907-.573 1.6524 1.6524 0 0 0-.807-.2099c-.2833 0-.5616.0724-.807.2098a1.5904 1.5904 0 0 0-.5907.5731L13.4642 9.99l-2.9954-5.0366a1.5913 1.5913 0 0 0-.591-.573 1.6533 1.6533 0 0 0-.8071-.2098c-.2834 0-.5617.0723-.8072.2097a1.5913 1.5913 0 0 0-.591.573L.2168 17.4808A1.5292 1.5292 0 0 0 0 18.2635c-.0001.2749.0744.545.216.783a1.59 1.59 0 0 0 .5908.5732c.2454.1375.5238.2098.8072.2098h5.6003c2.219 0 3.8554-.9454 4.9813-2.7899l2.7337-4.5922L16.3935 9.99l4.3944 7.382h-5.8586ZM7.123 17.3694l-3.9083-.0009 5.8586-9.8421 2.9232 4.921-1.9572 3.2892c-.7478 1.1967-1.5972 1.6328-2.9163 1.6328z",
  vite: "M13.056 23.238a.57.57 0 0 1-1.02-.355v-5.202c0-.63-.512-1.143-1.144-1.143H5.148a.57.57 0 0 1-.464-.903l3.777-5.29c.54-.753 0-1.804-.93-1.804H.57a.574.574 0 0 1-.543-.746.6.6 0 0 1 .08-.157L5.008.78a.57.57 0 0 1 .467-.24h14.589a.57.57 0 0 1 .466.903l-3.778 5.29c-.54.755 0 1.806.93 1.806h5.745c.238 0 .424.138.513.322a.56.56 0 0 1-.063.603z",
  rust: "M23.8346 11.7033l-1.0073-.6236a13.7268 13.7268 0 00-.0283-.2936l.8656-.8069a.3483.3483 0 00-.1154-.578l-1.1066-.414a8.4958 8.4958 0 00-.087-.2856l.6904-.9587a.3462.3462 0 00-.2257-.5446l-1.1663-.1894a9.3574 9.3574 0 00-.1407-.2622l.49-1.0761a.3437.3437 0 00-.0274-.3361.3486.3486 0 00-.3006-.154l-1.1845.0416a6.7444 6.7444 0 00-.1873-.2268l.2723-1.153a.3472.3472 0 00-.417-.4172l-1.1532.2724a14.0183 14.0183 0 00-.2278-.1873l.0415-1.1845a.3442.3442 0 00-.49-.328l-1.076.491c-.0872-.0476-.1742-.0952-.2623-.1407l-.1903-1.1673A.3483.3483 0 0016.256.955l-.9597.6905a8.4867 8.4867 0 00-.2855-.086l-.414-1.1066a.3483.3483 0 00-.5781-.1154l-.8069.8666a9.2936 9.2936 0 00-.2936-.0284L12.2946.1683a.3462.3462 0 00-.5892 0l-.6236 1.0073a13.7383 13.7383 0 00-.2936.0284L9.9803.3374a.3462.3462 0 00-.578.1154l-.4141 1.1065c-.0962.0274-.1903.0567-.2855.086L7.744.955a.3483.3483 0 00-.5447.2258L7.009 2.348a9.3574 9.3574 0 00-.2622.1407l-1.0762-.491a.3462.3462 0 00-.49.328l.0416 1.1845a7.9826 7.9826 0 00-.2278.1873L3.8413 3.425a.3472.3472 0 00-.4171.4171l.2713 1.1531c-.0628.075-.1255.1509-.1863.2268l-1.1845-.0415a.3462.3462 0 00-.328.49l.491 1.0761a9.167 9.167 0 00-.1407.2622l-1.1662.1894a.3483.3483 0 00-.2258.5446l.6904.9587a13.303 13.303 0 00-.087.2855l-1.1065.414a.3483.3483 0 00-.1155.5781l.8656.807a9.2936 9.2936 0 00-.0283.2935l-1.0073.6236a.3442.3442 0 000 .5892l1.0073.6236c.008.0982.0182.1964.0283.2936l-.8656.8079a.3462.3462 0 00.1155.578l1.1065.4141c.0273.0962.0567.1914.087.2855l-.6904.9587a.3452.3452 0 00.2268.5447l1.1662.1893c.0456.088.0922.1751.1408.2622l-.491 1.0762a.3462.3462 0 00.328.49l1.1834-.0415c.0618.0769.1235.1528.1873.2277l-.2713 1.1541a.3462.3462 0 00.4171.4161l1.153-.2713c.075.0638.151.1255.2279.1863l-.0415 1.1845a.3442.3442 0 00.49.327l1.0761-.49c.087.0486.1741.0951.2622.1407l.1903 1.1662a.3483.3483 0 00.5447.2268l.9587-.6904a9.299 9.299 0 00.2855.087l.414 1.1066a.3452.3452 0 00.5781.1154l.8079-.8656c.0972.0111.1954.0203.2936.0294l.6236 1.0073a.3472.3472 0 00.5892 0l.6236-1.0073c.0982-.0091.1964-.0183.2936-.0294l.8069.8656a.3483.3483 0 00.578-.1154l.4141-1.1066a8.4626 8.4626 0 00.2855-.087l.9587.6904a.3452.3452 0 00.5447-.2268l.1903-1.1662c.088-.0456.1751-.0931.2622-.1407l1.0762.49a.3472.3472 0 00.49-.327l-.0415-1.1845a6.7267 6.7267 0 00.2267-.1863l1.1531.2713a.3472.3472 0 00.4171-.416l-.2713-1.1542c.0628-.0749.1255-.1508.1863-.2278l1.1845.0415a.3442.3442 0 00.328-.49l-.49-1.076c.0475-.0872.0951-.1742.1407-.2623l1.1662-.1893a.3483.3483 0 00.2258-.5447l-.6904-.9587.087-.2855 1.1066-.414a.3462.3462 0 00.1154-.5781l-.8656-.8079c.0101-.0972.0202-.1954.0283-.2936l1.0073-.6236a.3442.3442 0 000-.5892zm-6.7413 8.3551a.7138.7138 0 01.2986-1.396.714.714 0 11-.2997 1.396zm-.3422-2.3142a.649.649 0 00-.7715.5l-.3573 1.6685c-1.1035.501-2.3285.7795-3.6193.7795a8.7368 8.7368 0 01-3.6951-.814l-.3574-1.6684a.648.648 0 00-.7714-.499l-1.473.3158a8.7216 8.7216 0 01-.7613-.898h7.1676c.081 0 .1356-.0141.1356-.088v-2.536c0-.074-.0536-.0881-.1356-.0881h-2.0966v-1.6077h2.2677c.2065 0 1.1065.0587 1.394 1.2088.0901.3533.2875 1.5044.4232 1.8729.1346.413.6833 1.2381 1.2685 1.2381h3.5716a.7492.7492 0 00.1296-.0131 8.7874 8.7874 0 01-.8119.9526zM6.8369 20.024a.714.714 0 11-.2997-1.396.714.714 0 01.2997 1.396zM4.1177 8.9972a.7137.7137 0 11-1.304.5791.7137.7137 0 011.304-.579zm-.8352 1.9813l1.5347-.6824a.65.65 0 00.33-.8585l-.3158-.7147h1.2432v5.6025H3.5669a8.7753 8.7753 0 01-.2834-3.348zm6.7343-.5437V8.7836h2.9601c.153 0 1.0792.1772 1.0792.8697 0 .575-.7107.7815-1.2948.7815zm10.7574 1.4862c0 .2187-.008.4363-.0243.651h-.9c-.09 0-.1265.0586-.1265.1477v.413c0 .973-.5487 1.1846-1.0296 1.2382-.4576.0517-.9648-.1913-1.0275-.4717-.2704-1.5186-.7198-1.8436-1.4305-2.4034.8817-.5599 1.799-1.386 1.799-2.4915 0-1.1936-.819-1.9458-1.3769-2.3153-.7825-.5163-1.6491-.6195-1.883-.6195H5.4682a8.7651 8.7651 0 014.907-2.7699l1.0974 1.151a.648.648 0 00.9182.0213l1.227-1.1743a8.7753 8.7753 0 016.0044 4.2762l-.8403 1.8982a.652.652 0 00.33.8585l1.6178.7188c.0283.2875.0425.577.0425.8717zm-9.3006-9.5993a.7128.7128 0 11.984 1.0316.7137.7137 0 01-.984-1.0316zm8.3389 6.71a.7107.7107 0 01.9395-.3625.7137.7137 0 11-.9405.3635z",
  go: "M1.811 10.231c-.047 0-.058-.023-.035-.059l.246-.315c.023-.035.081-.058.128-.058h4.172c.046 0 .058.035.035.07l-.199.303c-.023.036-.082.07-.117.07zM.047 11.306c-.047 0-.059-.023-.035-.058l.245-.316c.023-.035.082-.058.129-.058h5.328c.047 0 .07.035.058.07l-.093.28c-.012.047-.058.07-.105.07zm2.828 1.075c-.047 0-.059-.035-.035-.07l.163-.292c.023-.035.07-.07.117-.07h2.337c.047 0 .07.035.07.082l-.023.28c0 .047-.047.082-.082.082zm12.129-2.36c-.736.187-1.239.327-1.963.514-.176.046-.187.058-.34-.117-.174-.199-.303-.327-.548-.444-.737-.362-1.45-.257-2.115.175-.795.514-1.204 1.274-1.192 2.22.011.935.654 1.706 1.577 1.835.795.105 1.46-.175 1.987-.77.105-.13.198-.27.315-.434H10.47c-.245 0-.304-.152-.222-.35.152-.362.432-.97.596-1.274a.315.315 0 01.292-.187h4.253c-.023.316-.023.631-.07.947a4.983 4.983 0 01-.958 2.29c-.841 1.11-1.94 1.8-3.33 1.986-1.145.152-2.209-.07-3.143-.77-.865-.655-1.356-1.52-1.484-2.595-.152-1.274.222-2.419.993-3.424.83-1.086 1.928-1.776 3.272-2.02 1.098-.2 2.15-.07 3.096.571.62.41 1.063.97 1.356 1.648.07.105.023.164-.117.2m3.868 6.461c-1.064-.024-2.034-.328-2.852-1.029a3.665 3.665 0 01-1.262-2.255c-.21-1.32.152-2.489.947-3.529.853-1.122 1.881-1.706 3.272-1.95 1.192-.21 2.314-.095 3.33.595.923.63 1.496 1.484 1.648 2.605.198 1.578-.257 2.863-1.344 3.962-.771.783-1.718 1.273-2.805 1.495-.315.06-.63.07-.934.106zm2.78-4.72c-.011-.153-.011-.27-.034-.387-.21-1.157-1.274-1.81-2.384-1.554-1.087.245-1.788.935-2.045 2.033-.21.912.234 1.835 1.075 2.21.643.28 1.285.244 1.905-.07.923-.48 1.425-1.228 1.484-2.233z",
  dotnet:
    "M24 8.77h-2.468v7.565h-1.425V8.77h-2.462V7.53H24zm-6.852 7.565h-4.821V7.53h4.63v1.24h-3.205v2.494h2.953v1.234h-2.953v2.604h3.396zm-6.708 0H8.882L4.78 9.863a2.896 2.896 0 0 1-.258-.51h-.036c.032.189.048.592.048 1.21v5.772H3.157V7.53h1.659l3.965 6.32c.167.261.275.442.323.54h.024c-.04-.233-.06-.629-.06-1.185V7.529h1.372zm-8.703-.693a.868.829 0 0 1-.869.829.868.829 0 0 1-.868-.83.868.829 0 0 1 .868-.828.868.829 0 0 1 .869.829Z",
  django:
    "M11.146 0h3.924v18.166c-2.013.382-3.491.535-5.096.535-4.791 0-7.288-2.166-7.288-6.32 0-4.002 2.65-6.6 6.753-6.6.637 0 1.121.05 1.707.203zm0 9.143a3.894 3.894 0 00-1.325-.204c-1.988 0-3.134 1.223-3.134 3.365 0 2.09 1.096 3.236 3.109 3.236.433 0 .79-.025 1.35-.102V9.142zM21.314 6.06v9.098c0 3.134-.229 4.638-.917 5.937-.637 1.249-1.478 2.039-3.211 2.905l-3.644-1.733c1.733-.815 2.574-1.53 3.109-2.625.561-1.121.739-2.421.739-5.835V6.059h3.924zM17.39.021h3.924v4.026H17.39z",
  fastapi:
    "M12 .0387C5.3729.0384.0003 5.3931 0 11.9988c-.001 6.6066 5.372 11.9628 12 11.9625 6.628.0003 12.001-5.3559 12-11.9625-.0003-6.6057-5.3729-11.9604-12-11.96m-.829 5.4153h7.55l-7.5805 5.3284h5.1828L5.279 18.5436q2.9466-6.5444 5.892-13.0896",
  flask:
    "M10.773 2.878c-.013 1.434.322 4.624.445 5.734l-8.558 3.83c-.56-.959-.98-2.304-1.237-3.38l-.06.027c-.205.09-.406.053-.494-.088l-.011-.018-.82-1.506c-.058-.105-.05-.252.024-.392a.78.78 0 0 1 .358-.331l9.824-4.207c.146-.064.299-.063.4.004.106.062.127.128.13.327Zm.68 7c.523 1.97.675 2.412.832 2.818l-7.263 3.7a19.35 19.35 0 0 1-1.81-2.83l8.24-3.689Zm12.432 8.786h.003c.283.402-.047.657-.153.698l-.947.37c.037.125.035.319-.217.414l-.736.287c-.229.09-.398-.059-.42-.2l-.025-.125c-4.427 1.784-7.94 1.685-10.696.647-1.981-.745-3.576-1.983-4.846-3.379l6.948-3.54c.721 1.431 1.586 2.454 2.509 3.178 2.086 1.638 4.415 1.712 5.793 1.563l-.047-.233c-.015-.077.007-.135.086-.165l.734-.288a.302.302 0 0 1 .342.086l.748-.288a.306.306 0 0 1 .341.086l.583.89Z",
};
// Outline glyphs where a brand mark doesn't fit: plain PHP, Node.js, static files.
const OUTLINES: Record<string, string> = {
  php: "M8 7l-5 5 5 5M16 7l5 5-5 5M13.5 4.5l-3 15",
  node: "M12 2.8l8 4.6v9.2l-8 4.6-8-4.6V7.4z",
  static: "M6 3h8l4 4v14H6zM14 3v4h4M9 12h6M9 16h6",
};
export const icon = (id: string) =>
  BRANDS[id]
    ? `<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="${BRANDS[id]}"/></svg>`
    : `<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="${OUTLINES[id]}"/></svg>`;
const CHECK = `<svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
const CROSS = `<svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>`;

/** id, name, one line about it, group. */
export const TEMPLATES = [
  ["laravel", "Laravel", "Full-stack PHP framework", "PHP"],
  ["statamic", "Statamic", "Flat-file CMS on Laravel", "PHP"],
  ["symfony", "Symfony", "Modular PHP framework", "PHP"],
  ["php", "PHP", "Plain PHP, a public folder", "PHP"],
  ["next", "Next.js", "React with server rendering", "JavaScript"],
  ["nuxt", "Nuxt", "Vue with server rendering", "JavaScript"],
  ["vite", "Vite", "Frontend dev server and build", "JavaScript"],
  ["node", "Node.js", "A small HTTP server", "JavaScript"],
  ["django", "Django", "Batteries-included framework", "Python"],
  ["fastapi", "FastAPI", "Typed APIs on Uvicorn", "Python"],
  ["flask", "Flask", "Lightweight web framework", "Python"],
  ["rust", "Rust", "A Cargo HTTP server", "Other"],
  ["go", "Go", "A net/http server", "Other"],
  ["dotnet", ".NET", "ASP.NET Core web app", "Other"],
  ["static", "Static", "HTML files, no build", "Other"],
];
const GROUPS = ["PHP", "JavaScript", "Python", "Other"];

/** The steps creation goes through, as its log marks them (`==> Download`). */
export const stepsOf = (kind: string) =>
  ["laravel", "statamic", "symfony"].includes(kind)
    ? ["Download", "PHP dependencies", "JS dependencies", "Set up"]
    : ["next", "nuxt", "vite"].includes(kind)
      ? ["Download", "JS dependencies", "Set up"]
      : ["Set up"];

export let newKind = localStorage.getItem("newKind") ?? "laravel";
if (!TEMPLATES.some((t) => t[0] === newKind)) newKind = "laravel";

/* ---------- Template list ---------- */

const list = () => $("#new-kinds");
/** The templates shown, past the search. */
const items = (): HTMLElement[] => [...list().querySelectorAll(".kind-item:not([hidden])")] as HTMLElement[];

function pickKind(id: string) {
  newKind = id;
  localStorage.setItem("newKind", id);
  for (const it of list().querySelectorAll(".kind-item")) it.setAttribute("aria-selected", String(it.dataset.kind === id));
  const cur = items().find((it) => it.dataset.kind === id);
  if (cur) list().setAttribute("aria-activedescendant", cur.id);
  cur?.scrollIntoView?.({ block: "nearest" });
  loadOptions(id);
}

function drawKinds() {
  const box = list();
  box.replaceChildren();
  for (const g of GROUPS) {
    const group = el("div", "kind-group");
    group.setAttribute("role", "group");
    group.setAttribute("aria-label", g);
    group.append(el("div", "kind-head", g));
    for (const [id, name, about, of] of TEMPLATES) {
      if (of !== g) continue;
      const it = el("div", "kind-item");
      it.id = `kind-${id}`;
      it.dataset.kind = id;
      it.setAttribute("role", "option");
      const glyph = el("span", "kind-icon");
      glyph.innerHTML = icon(id);
      const text = el("span", "kind-text");
      text.append(el("span", "kind-name", name), el("span", "kind-about", about));
      const mark = el("span", "kind-check");
      mark.innerHTML = CHECK;
      it.append(glyph, text, mark);
      it.onclick = () => {
        pickKind(id);
        box.focus();
      };
      group.append(it);
    }
    box.append(group);
  }
  box.append(el("p", "kind-none", "No templates match"));
}

/** Only the templates whose name or line has the search in it; a group with none
 * hides, and a hidden choice moves to the first one shown. */
function filterKinds() {
  const q = $("#new-search").value.trim().toLowerCase();
  for (const it of list().querySelectorAll(".kind-item")) {
    const t = TEMPLATES.find((x) => x[0] === it.dataset.kind)!;
    it.hidden = !!q && !`${t[1]} ${t[2]}`.toLowerCase().includes(q);
  }
  for (const g of list().querySelectorAll(".kind-group")) g.hidden = !g.querySelector(".kind-item:not([hidden])");
  const shown = items();
  $(".kind-none", list()).hidden = shown.length > 0;
  if (shown.length && !shown.some((it) => it.dataset.kind === newKind)) pickKind(shown[0].dataset.kind!);
}

$("#new-search").oninput = filterKinds;
// Up and down from the search walk the templates shown; Enter goes on as in the list.
$("#new-search").onkeydown = (e: KeyboardEvent) => {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  e.preventDefault();
  const all = items();
  const i = all.findIndex((it) => it.dataset.kind === newKind);
  const next = all[i + (e.key === "ArrowDown" ? 1 : -1)];
  if (next) pickKind(next.dataset.kind!);
};
// Esc clears the search first, then closes.
$("#new-app").addEventListener(
  "keydown",
  (e: KeyboardEvent) => {
    const search = $("#new-search");
    if (e.key !== "Escape" || !search.value || $("#new-form").hidden) return;
    e.preventDefault();
    e.stopPropagation();
    search.value = "";
    filterKinds();
    search.focus();
  },
  true,
);

/** The option a key moves to: left/right along the list, up/down to the nearest
 * one in the row above or below (in list order where there is no layout). */
function neighbour(cur: HTMLElement, key: string) {
  const all = items();
  const i = all.indexOf(cur);
  const r = cur.getBoundingClientRect();
  const down = key === "ArrowDown" || key === "ArrowRight";
  if (key === "ArrowLeft" || key === "ArrowRight" || !r.height) return all[i + (down ? 1 : -1)];
  const rows = all.filter((x) => (down ? x.getBoundingClientRect().top > r.top + 2 : x.getBoundingClientRect().top < r.top - 2));
  if (!rows.length) return undefined;
  const tops = rows.map((x) => x.getBoundingClientRect().top);
  const y = down ? Math.min(...tops) : Math.max(...tops);
  return rows
    .filter((x) => x.getBoundingClientRect().top === y)
    .sort((a, b) => Math.abs(a.getBoundingClientRect().left - r.left) - Math.abs(b.getBoundingClientRect().left - r.left))[0];
}

$("#new-kinds").onkeydown = (e: KeyboardEvent) => {
  if (e.key === "Enter") {
    e.preventDefault();
    return void (items().length && $("#new-form").requestSubmit());
  }
  if (!e.key.startsWith("Arrow")) return;
  e.preventDefault();
  const cur = items().find((it) => it.dataset.kind === newKind) ?? items()[0];
  if (!cur) return;
  const next = neighbour(cur, e.key);
  if (next) pickKind(next.dataset.kind!);
};

/* ---------- Options ---------- */

type Opt = { key: string; label: string; choices: [string, string][]; default: string };
let opts: Opt[] = [];
let chosen: Record<string, string> = {};
const toggle = (o: Opt) => o.choices.length === 2 && o.choices[0][0] === "on" && o.choices[1][0] === "off";

/** The current choices in a line: "Livewire · SQLite · npm · PHP 8.4 · Git". */
export const summary = () =>
  opts
    .map((o) => (toggle(o) ? (chosen[o.key] === "on" ? o.label : null) : o.choices.find((c) => c[0] === chosen[o.key])?.[1]))
    .filter(Boolean)
    .join(" · ");

async function loadOptions(kind: string) {
  let got: Opt[] = [];
  try {
    got = (await invoke("create_options", { kind })) ?? [];
  } catch {
    // Options are extras: the template still works with its defaults.
  }
  if (kind !== newKind) return; // another template was picked meanwhile
  opts = got;
  const saved = JSON.parse(localStorage.getItem(`newOpts:${kind}`) ?? "{}");
  // HTTPS always starts from Settings › "HTTPS for new applications".
  const keep = (o: Opt) => o.key !== "https" && o.choices.some((c) => c[0] === saved[o.key]);
  chosen = Object.fromEntries(opts.map((o) => [o.key, keep(o) ? saved[o.key] : o.default]));
  drawOptions();
}

function setOpt(key: string, value: string) {
  chosen[key] = value;
  localStorage.setItem(`newOpts:${newKind}`, JSON.stringify(chosen));
  $("#new-opts-sum").textContent = summary();
}

function drawOptions() {
  $("#new-opts-sum").textContent = summary();
  const fields = el("div", "opt-fields");
  const checks = el("div", "opt-checks");
  for (const o of opts) {
    if (toggle(o)) {
      const label = el("label", "opt-check");
      const box = el("input");
      box.type = "checkbox";
      box.dataset.key = o.key;
      box.checked = chosen[o.key] === "on";
      box.onchange = () => setOpt(o.key, box.checked ? "on" : "off");
      label.append(box, document.createTextNode(o.label));
      checks.append(label);
      continue;
    }
    const label = el("label", "opt-field");
    const sel = el("select");
    sel.dataset.key = o.key;
    for (const [v, text] of o.choices) {
      const op = document.createElement("option");
      op.value = v;
      op.textContent = text;
      sel.append(op);
    }
    sel.value = chosen[o.key];
    sel.onchange = () => setOpt(o.key, sel.value);
    label.append(el("span", null, o.label), sel);
    fields.append(label);
  }
  const panel = el("div", "opt-panel");
  panel.append(fields, checks);
  $("#new-opts").replaceChildren(panel);
}

// Options open and close by height and fade (reduced motion just toggles). The sheet
// keeps its height: the template list gives Options its room and gets it back.
$("#new-options summary").onclick = (e: MouseEvent) => {
  const dlg = $("#new-app");
  const details = $("#new-options");
  const body = $("#new-opts");
  e.preventDefault();
  if (!dlg.style.height && dlg.offsetHeight) dlg.style.height = `${dlg.offsetHeight}px`;
  const done = (open: boolean) => {
    details.open = open;
    if (!open) dlg.style.height = "";
    items()
      .find((it) => it.dataset.kind === newKind)
      ?.scrollIntoView?.({ block: open ? "start" : "nearest" }); // the chosen one stays in view
  };
  if (!body.animate || matchMedia("(prefers-reduced-motion: reduce)").matches) return done(!details.open);
  if (body.getAnimations().length) return;
  const opening = !details.open;
  details.open = true;
  const h = `${body.offsetHeight}px`;
  body.style.overflow = "hidden";
  const run = body.animate(
    { height: opening ? ["0px", h] : [h, "0px"], opacity: opening ? [0, 1] : [1, 0] },
    { duration: 160, easing: "ease-out", fill: "forwards" },
  );
  run.onfinish = () => {
    done(opening);
    body.style.overflow = "";
    run.cancel(); // held at its end until closed, so nothing flashes back
  };
};

/* ---------- The sheet ---------- */

export function openNewApp() {
  const dlg = $("#new-app");
  // Closed while creating: back to its progress.
  if (job && !job.end) return dlg.showModal();
  drawKinds();
  $("#new-search").value = "";
  filterKinds();
  pickKind(newKind);
  const parent = $("#new-parent");
  parent.replaceChildren();
  for (const p of snap.paths) parent.add(new Option(tilde(p), p));
  parent.add(new Option("Choose a folder…", ""));
  parent.onchange = async () => {
    if (parent.value) return;
    const path = await window.__TAURI__.dialog.open({ directory: true, title: "Create the application in" });
    if (path) parent.add(new Option(tilde(path), path), 0);
    parent.selectedIndex = 0;
  };
  const name = $("#new-name");
  name.value = "";
  name.oninput = () => {
    name.value = name.value.toLowerCase().replace(/[^a-z0-9.-]/g, "-");
    preview();
  };
  preview();
  $("#new-options").open = false;
  $("#new-error").hidden = true;
  showForm(true);
  dlg.showModal();
  $("#new-search").focus();
}

/** "Its address: my-app.test", from what is typed (the placeholder until then). */
export function preview() {
  const name = $("#new-name");
  $("#new-url").replaceChildren("Its address: ", el("span", "new-addr", `${name.value || name.placeholder}.${snap.tld}`));
}

function showForm(on: boolean) {
  const dlg = $("#new-app");
  // The creation view takes the form's place at its size, so nothing jumps.
  dlg.style.height = !on && dlg.offsetHeight ? `${dlg.offsetHeight}px` : "";
  $("#new-form").hidden = !on;
  $("#new-progress").hidden = on;
}

$("#new-existing").onclick = async () => {
  if (await addAppFolder()) $("#new-app").close();
};
$("#new-cancel").onclick = () => $("#new-app").close();
$("#new-form").onsubmit = (e) => {
  e.preventDefault();
  const name = $("#new-name").value.trim();
  const parent = $("#new-parent").value;
  if (!name) return $("#new-name").focus();
  if (!parent || list().querySelector(".kind-none:not([hidden])")) return; // nothing matches the search
  create(newKind, name, parent);
};

/* ---------- Creating ---------- */

const term = termOut("console-out new-out");
term.out.dataset.empty = "Starting…";
$("#new-progress .new-term").append(term.out);

// The creation shown: what it makes and what its log said so far.
let job: { kind: string; name: string; site: string | null; from: number; raw: string; busy: boolean; end: "" | "ok" | "bad" } | null =
  null;

function setHead(state: "busy" | "ok" | "bad", title: string) {
  const mark = $("#new-state");
  mark.className = `new-state ${state}`;
  mark.innerHTML = state === "busy" ? `<span class="spin-dot"></span>` : state === "ok" ? CHECK : CROSS;
  $("#new-progress-title").textContent = title;
}

/** The steps as a checklist: done ✓, the current one spinning, failed ✕, the rest waiting. */
function drawSteps() {
  if (!job) return;
  const seen = [...job.raw.matchAll(/^==> (.+)$/gm)].map((m) => m[1].trim());
  const plan = stepsOf(job.kind);
  for (const s of seen) if (!plan.includes(s)) plan.push(s);
  const at = seen.length ? plan.indexOf(seen[seen.length - 1]) : job.end ? -1 : 0;
  $("#new-steps").replaceChildren(
    ...plan.map((s, i) => {
      const state =
        job!.end === "ok" || i < at ? "done" : i === at ? (job!.end === "bad" ? "bad" : "now") : job!.end === "bad" ? "skip" : "todo";
      const li = el("li", `new-step ${state}`);
      const mark = el("span", "new-step-mark");
      mark.innerHTML = state === "done" ? CHECK : state === "bad" ? CROSS : state === "now" ? `<span class="spin-dot"></span>` : "";
      li.append(mark, el("span", null, s));
      return li;
    }),
  );
}

const ERROR_LINE = /error|failed|fatal|exception|ERR!|could not|not found|✕|×/i;

/** The log as a terminal shows it; after a failure the lines that say what went wrong are marked. */
function drawLog() {
  if (!job) return;
  const text = clean(job.raw, true).replace(/^==> (.*)$/gm, "\u001b[1m==> $1\u001b[22m");
  if (job.end !== "bad") {
    term.out.replaceChildren(ansi(text));
    return term.follow();
  }
  const lines = text.replace(/\n+$/, "").split("\n");
  term.out.replaceChildren(
    ...lines.map((l, i) => {
      const line = el("span", ERROR_LINE.test(l) || i === lines.length - 1 ? "term-line term-error" : "term-line");
      line.append(ansi(l), "\n");
      return line;
    }),
  );
  term.follow();
}

async function readLog() {
  const j = job;
  if (!j || j.busy) return;
  j.busy = true;
  try {
    const page = await invoke("create_log", { name: j.name, from: j.from });
    if (page.end_offset < j.from) j.raw = ""; // a new run of the same name started over
    j.from = page.end_offset;
    if (page.text) j.raw += page.text;
  } catch {
    // No log yet.
  } finally {
    j.busy = false;
  }
  if (j === job) {
    drawSteps();
    drawLog();
  }
}

/** The log pane's size in characters, for the tools to fit their lines to (100 × 30
 * where it can't be measured). */
export function logSize() {
  const probe = el("span", null, "0".repeat(10));
  probe.style.cssText = "position:absolute;visibility:hidden";
  term.out.append(probe);
  const ch = probe.getBoundingClientRect().width / 10;
  const line = probe.getBoundingClientRect().height;
  probe.remove();
  const css = getComputedStyle(term.out);
  const width = term.out.clientWidth - parseFloat(css.paddingLeft) - parseFloat(css.paddingRight);
  const height = term.out.clientHeight - parseFloat(css.paddingTop) - parseFloat(css.paddingBottom);
  return {
    columns: ch > 0 && width > 0 ? Math.floor(width / ch) : 100,
    lines: line > 0 && height > 0 ? Math.floor(height / line) : 30,
  };
}

export async function create(kind: string, name: string, parent: string) {
  job = { kind, name, site: null, from: 0, raw: "", busy: false, end: "" };
  const j = job;
  showForm(false);
  setHead("busy", `Creating ${name}…`);
  term.out.replaceChildren();
  term.atEnd = true;
  $("#new-reveal").hidden = true;
  $("#new-done").textContent = "Open";
  $("#new-done").disabled = true;
  drawSteps();
  const timer = setInterval(readLog, 300);
  let error: string | null = null;
  try {
    j.site = await invoke("create_app", { kind, name, parent, options: { ...chosen }, ...logSize() });
  } catch (e) {
    error = String(e);
  }
  clearInterval(timer);
  while (j.busy) await new Promise((r) => setTimeout(r, 20));
  j.end = error ? "bad" : "ok";
  await readLog();
  if (j !== job) return;
  if (error && !j.from) {
    // Refused before it began (a name taken, a missing PHP): said in the form.
    job = null;
    showForm(true);
    $("#new-error").textContent = error;
    $("#new-error").hidden = false;
    return;
  }
  if (error) {
    // The log ends with the error; one that came from elsewhere is added.
    if (!j.raw.includes(error)) j.raw += `${j.raw.endsWith("\n") ? "" : "\n"}${error}\n`;
    drawLog();
    setHead("bad", `Couldn't create ${name}`);
    $("#new-reveal").hidden = false;
    $("#new-done").textContent = "Close";
  } else {
    setHead("ok", `${name} is ready`);
    await call("state");
  }
  $("#new-done").disabled = false;
  $("#new-done").focus({ focusVisible: false } as FocusOptions);
}

/** The new app, selected and in view in the sidebar, past any filter that hid it. */
export function reveal(site: string) {
  if (mode !== "apps") setMode("apps");
  if (!$(`#list .item[data-name="${CSS.escape(site)}"]`)) {
    const f = filterOf("apps");
    f.kind = null;
    f.running = false;
    localStorage.setItem("filters", JSON.stringify(filters));
    const search = $("#search");
    if (search.value) {
      search.value = "";
      search.dispatchEvent(new Event("input"));
    }
  }
  select(site);
}

$("#new-done").onclick = () => {
  const site = job?.end === "ok" ? job.site : null;
  $("#new-app").close();
  if (site) reveal(site);
};
$("#new-reveal").onclick = () => job && invoke("create_reveal", { name: job.name });

late.openNewApp = openNewApp;
